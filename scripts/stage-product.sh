#!/usr/bin/env bash
# Shared XCOS payload staging for formal releases and a future candidate build.
# Caller owns a fresh private work directory and source-bound build metadata.
stage_xcos_payload() {
local source_root="$1" work="$2" server_binary="$3" source_revision="$4"
local product=xcos media_version media_hash
mkdir -p "$work/stage/bin" "$work/stage/share/systemd" "$work/stage/config"
install -m 0755 "$server_binary" "$work/stage/bin/$product"
python3 - "$server_binary.xssi-web-build.json" "$work/stage/bin/$product" "$product" "$source_revision" <<'RECEIPT'
import hashlib,json,pathlib,sys
receipt_file,binary_file,product,revision=sys.argv[1:]
receipt=json.loads(pathlib.Path(receipt_file).read_bytes())
identity=receipt['identity']
if receipt['format']!='xssi-web-build-receipt-v1' or identity['product']!=product or identity['source_revision']!=revision or identity['target']!='x86_64-unknown-linux-gnu':
    raise SystemExit('Staged binary differs from its source-bound build receipt')
with open(binary_file,'rb') as stream: digest=hashlib.file_digest(stream,'sha256').hexdigest()
if digest!=receipt['binary_sha256']: raise SystemExit('Staged binary was replaced after verification')
RECEIPT
install -m 0644 "$work/metadata/$product-state-contract.json" "$work/stage/share/state-contract.json"
install -d -m 0755 "$work/stage/share/contracts"
cp -a -- "$work/metadata/contracts/." "$work/stage/share/contracts/"
install -m 0644 "$work/metadata/$product-web-assets.json" "$work/stage/share/web-assets.json"
install -m 0644 "$source_root/LICENSE-APACHE" "$work/stage/share/LICENSE"
install -m 0644 "$source_root/config/xcos.json.example" "$work/stage/config/config.json.example"
install -m 0644 "$source_root/config/mediamtx.env.example" "$work/stage/config/mediamtx.env.example"
install -m 0644 "$source_root/config/mediamtx.lock" "$work/stage/share/mediamtx.lock"
install -m 0644 "$source_root/config/mediamtx.yml" "$work/stage/share/mediamtx.yml"
media_version="$(sed -n 's/^version=//p' "$source_root/config/mediamtx.lock")"
media_hash="$(sed -n 's/^sha256=//p' "$source_root/config/mediamtx.lock")"
[[ "$media_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ && "$media_hash" =~ ^[0-9a-f]{64}$ ]]
curl --fail --location --proto '=https' --proto-redir '=https' --max-filesize 83886080 --max-time 600 --retry 3 --output "$work/media.tar.gz" "https://github.com/bluenviron/mediamtx/releases/download/$media_version/mediamtx_${media_version}_linux_amd64.tar.gz"
python3 - "$work/media.tar.gz" "$work" <<'MEDIA'
import pathlib,sys,tarfile
destination=pathlib.Path(sys.argv[2]);expected={'mediamtx':64*1024*1024,'LICENSE':65536}
with tarfile.open(sys.argv[1],'r:gz') as archive:
    selected={};count=0;expanded=0
    for item in archive:
        count+=1;expanded+=item.size
        name=pathlib.PurePosixPath(item.name)
        if count>128 or item.size<0 or expanded>80*1024*1024 or name.is_absolute() or '..' in name.parts or not (item.isfile() or item.isdir()):
            raise SystemExit('MediaMTX archive contains uncontrolled entries or exceeds its bound')
        if item.name not in expected:continue
        if item.name in selected or not item.isfile() or not 0<item.size<=expected[item.name]:
            raise SystemExit('Uncontrolled MediaMTX payload entry')
        selected[item.name]=item
    if set(selected)!=set(expected):raise SystemExit('Incomplete MediaMTX payload')
    for name,item in selected.items():
        with archive.extractfile(item) as source,(destination/name).open('xb') as output:
            content=source.read(item.size+1)
            if len(content)!=item.size:raise SystemExit('MediaMTX payload size differs')
            output.write(content)
(destination/'mediamtx').chmod(0o755)
MEDIA
[[ "$(sha256sum "$work/mediamtx" | cut -d ' ' -f 1)" = "$media_hash" && "$("$work/mediamtx" --version)" = "$media_version" ]]
install -m 0755 "$work/mediamtx" "$work/stage/bin/mediamtx"
install -m 0644 "$work/LICENSE" "$work/stage/share/MediaMTX-LICENSE"
# The original vendor archive is required for XSSI's external-component receipt.
export XSSI_MEDIA_ARCHIVE="$work/media.tar.gz"
}
