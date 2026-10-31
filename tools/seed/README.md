# The public documents seed peer

`cadrs --public-seed` in a container: it stays online in the public room, keeps every listing,
star and published document, and so keeps documents reachable while their owners are offline
(`cadrs_net::public`). Its identity (and so its cadrs ID) and what it keeps are in `/data`.

cadrs's own seed runs on the Unraid server (cadrs ID
`a560c0cfdeaac54cde945476f851486bbdf584adc280c4120616b06a6b10ba6f`, in
`cadrs_net::public::DEFAULT_SEEDS`), from `/mnt/user/appdata/cadrs-seed`. To update it to a new
build (the image is Ubuntu 26.04, as the build machine, for its glibc):

```sh
cargo build -r && strip -o /tmp/cadrs target/release/cadrs
scp /tmp/cadrs tools/seed/Dockerfile root@<server>:/mnt/user/appdata/cadrs-seed/build/
ssh root@<server> 'cd /mnt/user/appdata/cadrs-seed/build && docker build -t cadrs-seed . \
  && docker rm -f cadrs-seed && docker run -d --name cadrs-seed --restart unless-stopped \
  --network host -e RUST_LOG=info,cadrs_net::public=debug \
  -v /mnt/user/appdata/cadrs-seed/data:/data cadrs-seed'
```

`docker logs cadrs-seed` shows its cadrs ID, the peers it meets and the revisions it keeps.
