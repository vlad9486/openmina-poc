# Mina Transport

`mina-transport` is a thin wrapper around the libp2p networking stack that provides a message passing interface to libp2p. It is designed to make it easier to talk to Mina node in your own applications. It configures libp2p to use exactly the same transport as Mina node is using.

## Usage

Spawn a new libp2p swarm to run on the berkeley chain. Specify proper `chain_id`, specify address where to listen new connections and list some peers to bootstrap with.

```rust
    let local_key = mina_transport::generate_identity();
    let chain_id = b"8c4908f1f873bd4e8a52aeb4981285a148914a51e61de6ac39180e61d0144771";
    let listen_on = "/ip4/0.0.0.0/tcp/8302".parse().unwrap();
    let peers = [
        "/dns4/seed-1.berkeley.o1test.net/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs".parse().unwrap(),
    ];
    let mut swarm = mina_transport::run(local_key, chain_id, listen_on, peers);
```

And then wait for events. It must be used with `tokio` v1 runtime.

```rust
    while let Some(event) = swarm.next().await {
        match event {
            ...
        }
    }
```

See `examples/simple.rs` for details.
