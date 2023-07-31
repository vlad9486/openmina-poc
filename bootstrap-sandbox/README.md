# Tools for exploring and testing Mina bootstrap 

Work in progress, the replay tool is slightly unstable.

### Log

The crate uses `env_logger`, set `RUST_LOG=info` variable to see logs.

#### Record:

```
cargo run --bin bootstrap-sandbox --release -- record
```

This will create bunch of files and directories in `target` directory.

#### Record with bootstrap

The tool can bootstrap itself while recording:

```
cargo run --bin bootstrap-sandbox --release -- record --bootstrap
```

Might be useful to test our `mina-tree` implementation.

#### Bootstrap again

Bootstrap itself from the stored record on disk. It doesn't use network at all.

```
cargo run --bin bootstrap-sandbox --release -- again $BLOCK_HEIGHT
```

#### Replay

Replay stored record to the peer. Can bootstrap OCaml node.

```
cargo run --bin bootstrap-sandbox --release -- replay $BLOCK_HEIGHT
```

Will print in log its peer_id, and addresses where it is listening for example:

```
[2023-07-31T09:31:34Z INFO  bootstrap_sandbox] 12D3KooWETkiRaHCdztkbmrWQTET9HMWimQPx5sH5pLSRZNxRsjw
[2023-07-31T09:31:59Z INFO  bootstrap_sandbox::replay_new] listen on /ip4/127.0.0.1/tcp/8302
[2023-07-31T09:31:59Z INFO  bootstrap_sandbox::replay_new] listen on /ip4/192.168.0.205/tcp/8302
```

Run the Rust or OCaml Mina node specifying the peer. For local network it will be:
```
--peer /ip4/192.168.0.205/tcp/8302/p2p/12D3KooWK4K1gpTQWqpcwjcnffMZmx5MMBoEL7oz7i6czmt1L6ZE
```

This will replay bootstrap of the block at specified height.

#### See available records:

```
ls target/record
```
