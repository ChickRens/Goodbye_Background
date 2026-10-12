import numpy as np
import os

# Load image_pe from a .npy file or a raw file named "image_pe"
if os.path.exists("image_pe.npy"):
    image_pe = np.load("image_pe.npy")
elif os.path.exists("image_pe"):
    try:
        image_pe = np.load("image_pe")
    except Exception:
        image_pe = np.fromfile("image_pe")
else:
    raise FileNotFoundError("image_pe or image_pe.npy not found")

image_pe = image_pe.astype(np.float32)
image_pe.tofile("image_pe.bin")

