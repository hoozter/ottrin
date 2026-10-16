"""Generate the compact, owned input fixtures for packaged preview GUI QA."""
from pathlib import Path
import io
from PIL import Image

def create(root: Path):
    root=Path(root);assert root.is_dir()
    model=root/'model.ply';affinity=root/'composite.afphoto'
    assert not model.exists() and not affinity.exists()
    model.write_text('ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 4\nproperty list uchar int vertex_indices\nend_header\n0 0 0 220 60 140\n1 0 0 220 60 140\n0 1 0 220 60 140\n0 0 1 220 60 140\n3 0 2 1\n3 0 1 3\n3 0 3 2\n3 1 2 3\n')
    image=io.BytesIO();Image.new('RGB',(512,320),(20,190,80)).save(image,format='PNG')
    affinity.write_bytes(b'OTTRIN OWNED EMBEDDED PREVIEW FIXTURE\0'+b' '*128+image.getvalue()+b'\0tail')
    eps=root/'drawing.eps'
    assert not eps.exists()
    eps.write_text('%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 320 180\n0 0.8 0.2 setrgbcolor 10 10 300 160 rectfill\n0.9 0.05 0.05 setrgbcolor newpath 160 90 40 0 360 arc fill\nshowpage\n%%EOF\n')
    return model,affinity,eps
