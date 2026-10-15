"""pack.py <folder> <out.tempest>: CHECKSUMS + zip in Tempest.CLI `mod pack` format."""
import hashlib, os, sys, zipfile

stage, outzip = sys.argv[1], sys.argv[2]
files = sorted(
    os.path.relpath(os.path.join(d, f), stage).replace(os.sep, "/")
    for d, _, fs in os.walk(stage)
    for f in fs
    if f not in ("CHECKSUMS", "CHECKSUMS.asc")
)
lines = []
for rel in files:
    with open(os.path.join(stage, *rel.split("/")), "rb") as fh:
        lines.append(f"{hashlib.sha256(fh.read()).hexdigest()} {rel}")
with open(os.path.join(stage, "CHECKSUMS"), "w", newline="\n") as fh:
    fh.write("\n".join(lines))
if os.path.exists(outzip):
    os.remove(outzip)
with zipfile.ZipFile(outzip, "w", zipfile.ZIP_DEFLATED) as z:
    for d, _, fs in os.walk(stage):
        for f in fs:
            full = os.path.join(d, f)
            z.write(full, os.path.relpath(full, stage).replace(os.sep, "/"))
