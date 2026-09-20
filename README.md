# bee

Rust CLI for a Hive. Layout, dest lanes, tracker notes, and doc-store verbs. Node is not required.

Config lives under `.hivemind/`. Old bins `hivemind`, `odm`, and `heio` print a rename and forward to `bee`.

```sh
curl -fsSL https://github.com/hembrow-innovations/bee/releases/latest/download/install.sh | sh
```

```sh
cargo test --workspace
cargo build --release
```
