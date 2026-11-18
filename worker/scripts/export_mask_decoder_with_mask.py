"""SAM2.1 small の動画用マスクデコーダを、前回のマスク（mask_input / has_mask）を受け取れる形で ONNX に書き出す。

仕様: docs/superpowers/specs/2026-11-16-video-prompt-mask-input-design.md
本番は diffusionstudio/sam2.1-small-video-onnx-fp16 の 4 グラフ（vision_encoder・memory_encoder・memory_attention・
pointer_tpos）と、このスクリプトで書き出したマスクデコーダ（mask_decoder_with_mask_v1.onnx）を組み合わせて使う。

入出力は diffusionstudio の mask_decoder.onnx と同じ名前・形に、次を足したもの:
  入力  mask_input [1,1,256,256] float32（前回の low_res_mask。-32〜32 に丸めたもの）, has_mask [1] float32（1 で使う）
  出力  candidate_masks [1,4,256,256]（object score で打ち消す前の 4 つの候補）, candidate_ious [1,4]
選び方（transformers の Sam2VideoModel と同じ）:
  - 本物の点（label != -1）が 1 つ以下: 候補 1〜3 のうち IoU 予測が最大のもの（object pointer もその候補のトークン）
  - 2 つ以上: 候補 0。安定度が 0.98 未満なら候補 1〜3 の最大に切り替える（object pointer は候補 0 のトークン）
  - object_score_logits <= 0 ならマスクを -1024、object pointer を no_object_pointer にする

書き出した環境（2026-11-16）: torch 2.15.0・transformers 5.17.0・opset 17・fp32。ワーカーの依存には入れない
（書き出すときだけ使い捨ての venv に入れる）。チェックポイントは
https://huggingface.co/facebook/sam2.1-hiera-small/resolve/main/ の config.json と model.safetensors を curl で落とした
ディレクトリを渡す（xet の配布元に繋がらない環境があるため、from_pretrained にリポジトリ名を直接渡さない）。

検証（has_mask=0 で diffusionstudio 版と比較。動画のフレーム 0 の実特徴量）: マスク IoU 0.9986〜1.0、object score・IoU 予測は
小数 2 桁で一致、object pointer の cos 類似度 1.0、追跡のダミー点（label -1）は IoU 1.0。差は diffusionstudio が fp16 のため。
本番のファイルの SHA-256 は cutout/models.py の MASK_DECODER_SHA256（書き出し直すと値が変わり得るので、変えたら両方の値を直す）。

使い方: python export_mask_decoder_with_mask.py <チェックポイントのディレクトリ> <出力する .onnx>
"""

import sys

import torch
import torch.nn.functional as F
from transformers import Sam2VideoModel

NO_OBJ_SCORE = -1024.0


class DecoderWithMask(torch.nn.Module):
    def __init__(self, model: Sam2VideoModel):
        super().__init__()
        self.m = model
        self.register_buffer("image_pe", model.get_image_wide_positional_embeddings().detach())

    def _decode_all(self, image_embeddings, dense, sparse, feat_s0, feat_s1):
        md = self.m.mask_decoder
        output_tokens = torch.cat([md.obj_score_token.weight, md.iou_token.weight, md.mask_tokens.weight], dim=0)
        output_tokens = output_tokens.repeat(1, 1, 1, 1)
        tokens = torch.cat((output_tokens, sparse), dim=2)
        image_embeddings = image_embeddings + dense
        point_embeddings, image_embeddings = md.transformer(
            point_embeddings=tokens,
            image_embeddings=image_embeddings,
            image_positional_embeddings=self.image_pe,
            attention_similarity=None,
            target_embedding=None,
        )
        iou_token_out = point_embeddings[:, :, 1, :]
        mask_tokens_out = point_embeddings[:, :, 2 : (2 + md.num_mask_tokens), :]
        image_embeddings = image_embeddings.transpose(2, 3).view(1, 256, 64, 64)
        up = md.upscale_conv1(image_embeddings) + feat_s1
        up = md.activation(md.upscale_layer_norm(up))
        up = md.activation(md.upscale_conv2(up) + feat_s0)
        hyper_in = torch.stack(
            [md.output_hypernetworks_mlps[i](mask_tokens_out[:, :, i, :]) for i in range(md.num_mask_tokens)], dim=2
        )
        _, c, h, w = up.shape
        masks = (hyper_in @ up.view(1, 1, c, h * w)).view(1, 1, -1, h, w)  # [1,1,4,256,256]
        iou_pred = md.iou_prediction_head(iou_token_out)  # [1,1,4]
        obj = md.pred_obj_score_head(point_embeddings[:, :, 0, :])  # [1,1,1]
        return masks, iou_pred, mask_tokens_out, obj

    def forward(self, feats0, feats1, feats2_cond, input_points, input_labels, mask_input, has_mask):
        pe = self.m.prompt_encoder
        sparse = pe._embed_points(input_points, input_labels, pad=True)
        dense_no = pe.no_mask_embed.weight.reshape(1, -1, 1, 1).expand(1, -1, 64, 64)
        dense_m = pe.mask_embed(mask_input)
        hm = has_mask.reshape(1, 1, 1, 1)
        dense = hm * dense_m + (1.0 - hm) * dense_no

        masks, ious, tokens, obj = self._decode_all(feats2_cond, dense, sparse, feats0, feats1)
        md = self.m.mask_decoder
        multi_masks, multi_ious = masks[:, :, 1:], ious[:, :, 1:]
        best = torch.argmax(multi_ious, dim=-1)  # [1,1]
        best_mask = torch.gather(multi_masks, 2, best[..., None, None, None].expand(-1, -1, 1, 256, 256))
        best_iou = torch.gather(multi_ious, 2, best[..., None])
        best_tok = torch.gather(tokens[:, :, 1:], 2, best[..., None, None].expand(-1, -1, 1, 256))

        single = masks[:, :, 0:1]
        flat = single.flatten(-2)
        d = md.dynamic_multimask_stability_delta
        area_i = (flat > d).sum(-1).float()
        area_u = (flat > -d).sum(-1).float()
        stab = torch.where(area_u > 0, area_i / area_u, torch.ones_like(area_u))
        stable = stab >= md.dynamic_multimask_stability_thresh  # [1,1,1]
        single_sel = torch.where(stable[..., None, None], single, best_mask)
        single_iou = torch.where(stable, ious[:, :, 0:1], best_iou)

        n_real = (input_labels != -1).sum()
        use_multi = n_real <= 1
        low = torch.where(use_multi, best_mask, single_sel)  # [1,1,1,256,256]
        iou = torch.where(use_multi, best_iou, single_iou)
        tok = torch.where(use_multi, best_tok, tokens[:, :, 0:1])

        appear = obj > 0  # [1,1,1]
        low = torch.where(appear[..., None, None], low, torch.full_like(low, NO_OBJ_SCORE))
        low = low[:, :, 0]  # [1,1,256,256]
        high = F.interpolate(low, size=(1024, 1024), mode="bilinear", align_corners=False)
        ptr = self.m.object_pointer_proj(tok[:, :, 0])  # [1,1,256]
        lam = appear.to(ptr.dtype)
        ptr = lam * ptr + (1 - lam) * self.m.no_object_pointer
        return low, high, iou.reshape(1, 1), obj, ptr, masks[:, 0], ious[:, 0]


def main(checkpoint_dir: str, out_path: str):
    model = Sam2VideoModel.from_pretrained(checkpoint_dir).eval()
    wrapper = DecoderWithMask(model).eval()
    args = (
        torch.randn(1, 32, 256, 256),
        torch.randn(1, 64, 128, 128),
        torch.randn(1, 256, 64, 64),
        torch.tensor([[[[500.0, 400.0], [300.0, 700.0]]]]),
        torch.tensor([[[1, 1]]], dtype=torch.int32),
        torch.zeros(1, 1, 256, 256),
        torch.zeros(1),
    )
    names_in = ["feats0", "feats1", "feats2_cond", "input_points", "input_labels", "mask_input", "has_mask"]
    names_out = [
        "low_res_mask", "high_res_mask", "iou", "object_score_logits", "object_pointer",
        "candidate_masks", "candidate_ious",
    ]
    with torch.no_grad():
        torch.onnx.export(
            wrapper, args, out_path, input_names=names_in, output_names=names_out, opset_version=17,
            dynamic_axes={"input_points": {2: "num_points"}, "input_labels": {2: "num_points"}}, dynamo=False,
        )
    print("exported", out_path)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    main(sys.argv[1], sys.argv[2])
