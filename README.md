<h2 align="center">
    <a>Gramophone</a>
</h2>

> [!CAUTION]
> _Gramophone is still an experimental project._
>
> _Please avoid using this in your projects in production. Bugs and unexpected output/logic with these crates will occur at any time._
>
> _If you wish to use it in your production project, **PLEASE USE IT AT YOUR OWN RISK!**_

Gramophone is an ecosystem of Rust libraries made for the Discord Voice API.

Unlike [songbird](https://github.com/serenity-rs/songbird), a well-known Discord voice system crate that supports for [Serenity](https://github.com/serenity-rs/serenity) and [Twilight](https://github.com/twilight-rs/twilight), this crate is **only made** for the [Twilight ecosystem](https://github.com/twilight-rs/twilight).

## Crates

### [`gramophone`](./src)
This is the main crate of the Gramophone ecosystem that implements based on the specifications of the Discord's Voice API (except the DAVE protocol). It also provides modular components (inspired from [`twilight-gateway`](https://crates.io/crates/twilight-gteway)) that allow advanced users to take full control of the voice connection logic.

### `gramophone-easy` (soon)
Since the main crate (gramophone) is too primitive for users who are new to Rust, this crate provides abstraction layer for users who wanted to use Gramophone without handling any manual voice connection logic work under the hood.

### [`gramophone-types`](./crates/types)
This crate contains most of the types needed gathered from Discord's Voice API.

## Example
As of writing this file, the project is in work in progress (WIP) and the entire API for [`gramophone`](./src) have not been finalized yet. You may want to see the usage of this ecosystem by heading to the [`example/usage.rs`](./examples/usage.rs) file.

## License
The license of the main crate, [`gramophone`](./Cargo.toml) is licensed under [Mozilla Public License Version 2.0](./LICENSE.txt).

The rest of the [crates](./crates) available in this repository excluding the main crate ([`gramophone`](./Cargo.toml)) are all licensed under the [ISC License](./crates/types/LICENSE.txt).
