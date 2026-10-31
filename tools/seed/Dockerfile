# The cadrs public documents seed peer (cadrs --public-seed): stays online in the public room
# and keeps every listing, star and published document. Its identity and what it keeps are in
# /data (mount /mnt/user/appdata/cadrs-seed/data).
FROM ubuntu:26.04
RUN apt-get update \
 && apt-get install -y --no-install-recommends libudev1 libwayland-client0 libasound2t64 libffi8 ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY cadrs /usr/local/bin/cadrs
ENV RUST_LOG=info
ENTRYPOINT ["/usr/local/bin/cadrs", "--public-seed", "--data-dir", "/data"]
