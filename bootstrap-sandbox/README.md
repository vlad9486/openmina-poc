# Tools for exploring and testing Mina bootstrap 

Work in progress, the replay tool is slightly unstable.

#### Record: 

```
cargo run --bin bootstrap-sandbox --release -- --record
```

This will create bunch of files and directories in `target` directory.

#### Replay: 

```
cargo run --bin bootstrap-sandbox --release -- --replay $BLOCK_HEIGHT
```

Will print in log its peer_id, and addresses where it is listening for example:

```
[2023-07-25T11:27:56Z INFO  bootstrap_sandbox] 12D3KooWK4K1gpTQWqpcwjcnffMZmx5MMBoEL7oz7i6czmt1L6ZE
[2023-07-25T11:28:21Z INFO  mina_rpc::state] listen ListenerId(5362115367412634051) on /ip4/127.0.0.1/tcp/8302
[2023-07-25T11:28:21Z INFO  mina_rpc::state] listen ListenerId(5362115367412634051) on /ip4/192.168.0.205/tcp/8302
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
