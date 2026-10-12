import sys
import os

base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

sys.path.append(os.path.join(base_dir, "SAM2"))
print(sys.path)

from sam2.build_sam import build_sam2
import torch
import onnx
import onnxruntime as ort
import numpy as np
import torch.nn as nn

# raise Exception("Stop here to check versions")

print("PyTorch version:", torch.__version__)
print("ONNX version:", onnx.__version__)
print("ONNX Runtime version:", ort.__version__)

# Load a SAM2 model from .pt file 
config_path = "configs/sam2.1/sam2.1_hiera_s.yaml"
model_path = r"models/sam2.1_hiera_small.pt"
model = build_sam2(config_path, model_path, device='cpu')  # Adjust the device as needed
# model = torch.load(model_path, map_location=torch.device('cpu'))
# model = models.resnet18(pretrained=True)  # Replace with the actual model architecture if needed
model.eval()

print(model)
image_pe = model.sam_prompt_encoder.get_dense_pe()

np.save(
    "image_pe.npy",
    image_pe.cpu().numpy()
)

# class ImageEncoderWrapper(torch.nn.Module):
#     def __init__(self, model):
#         super().__init__()

#         self.image_encoder = model.image_encoder

#     def forward(self, image):

#         backbone_out = self.image_encoder(image)

#         print(type(backbone_out))

#         if isinstance(backbone_out, dict):
#             print(backbone_out.keys())

#         image_embeddings = backbone_out["vision_features"]

#         high_res_feats = backbone_out["backbone_fpn"]

#         return (
#             image_embeddings,
#             high_res_feats[0],
#             high_res_feats[1],
#         )

class MaskDecoderWrapper(nn.Module):
    def __init__(self, mask_decoder):
        super().__init__()
        self.mask_decoder = mask_decoder

    def forward(
        self,
        image_embeddings,
        image_pe,
        sparse_prompt_embeddings,
        dense_prompt_embeddings,
        high_res_feature_s0,
        high_res_feature_s1,
    ):
        return self.mask_decoder(
            image_embeddings=image_embeddings,
            image_pe=image_pe,
            sparse_prompt_embeddings=sparse_prompt_embeddings,
            dense_prompt_embeddings=dense_prompt_embeddings,
            multimask_output=False,
            repeat_image=False,
            high_res_features=(
                high_res_feature_s0,
                high_res_feature_s1,
            ),
        )


wrapper = MaskDecoderWrapper(model.sam_mask_decoder)

# raise

# Dummy inputs for mask_decoder
image_embeddings = torch.zeros((1, 256, 64, 64), dtype=torch.float32)
image_pe = torch.zeros((1, 256, 64, 64), dtype=torch.float32)
sparse_prompt_embeddings = torch.zeros((1, 4, 256), dtype=torch.float32)
dense_prompt_embeddings = torch.zeros((1, 256, 64, 64), dtype=torch.float32)
# high_res_features are added to the upscaled embeddings from mask_decoder
# output_upscaling: 256 -> 64 -> 32 channels with 2x upsampling each time
high_res_feature_s0 = torch.zeros((1, 32, 256, 256), dtype=torch.float32)
high_res_feature_s1 = torch.zeros((1, 64, 128, 128), dtype=torch.float32)
multimask_output = False
repeat_image = False

onnx_model_path = r"sam2.1_hiera_small_mask_decoder.onnx"

try:
    print("Attempting direct torch.onnx.export for mask_decoder...")
    torch.onnx.export(
        wrapper,
        (
            image_embeddings,
            image_pe,
            sparse_prompt_embeddings,
            dense_prompt_embeddings,
            high_res_feature_s0,
            high_res_feature_s1,
        ),
        onnx_model_path,
        opset_version=18,
        input_names=[
            "image_embeddings",
            "image_pe",
            "sparse_prompt_embeddings",
            "dense_prompt_embeddings",
            "high_res_feature_s0",
            "high_res_feature_s1",
        ],
        output_names=[
            "masks",
            "iou_pred",
            "sam_tokens_out",
            "object_score_logits",
        ],
        dynamic_axes={
            "sparse_prompt_embeddings": {
                1: "num_points"
            }
        },
        dynamo=False,
    )
except Exception as e:
    print(f"mask_decoder export failed: {e}")
    raise

print(f"Model exported to {onnx_model_path}")
onnx_model = onnx.load(onnx_model_path)
onnx.checker.check_model(onnx_model)
print("ONNX model is valid.")
ort_session = ort.InferenceSession(onnx_model_path)
input_names = [inp.name for inp in ort_session.get_inputs()]
print(f"Input names: {input_names}")
ort_inputs = {
    input_names[0]: image_embeddings.numpy(),
    input_names[1]: image_pe.numpy(),
    input_names[2]: sparse_prompt_embeddings.numpy(),
    input_names[3]: dense_prompt_embeddings.numpy(),
    input_names[4]: high_res_feature_s0.numpy(),
    input_names[5]: high_res_feature_s1.numpy(),
}
ort_outputs = ort_session.run(None, ort_inputs)
print("Inference output shapes:")
for i, output in enumerate(ort_outputs):
    print(f"  Output {i}: shape={output.shape}, dtype={output.dtype}")
print("SUCCESS: mask_decoder exported and inference successful!")