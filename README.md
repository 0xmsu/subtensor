```commandline             _      _
███████╗██╗   ██╗██████╗ ████████╗███████╗███╗   ██╗███████╗ ██████╗ ██████╗
██╔════╝██║   ██║██╔══██╗╚══██╔══╝██╔════╝████╗  ██║██╔════╝██╔═══██╗██╔══██╗
███████╗██║   ██║██████╔╝   ██║   █████╗  ██╔██╗ ██║███████╗██║   ██║██████╔╝
╚════██║██║   ██║██╔══██╗   ██║   ██╔══╝  ██║╚██╗██║╚════██║██║   ██║██╔══██╗
███████║╚██████╔╝██████╔╝   ██║   ███████╗██║ ╚████║███████║╚██████╔╝██║  ██║
╚══════╝ ╚═════╝ ╚═════╝    ╚═╝   ╚══════╝╚═╝  ╚═══╝╚══════╝ ╚═════╝ ╚═╝  ╚═╝

```

# **Subtensor** <!-- omit in toc -->
[![CodeQL](https://github.com/opentensor/subtensor/actions/workflows/github-code-scanning/codeql/badge.svg)](https://github.com/opentensor/subtensor/actions)
[![Discord Chat](https://img.shields.io/discord/308323056592486420.svg)](https://discord.gg/bittensor)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

This repository contains Bittensor's substrate-chain. Subtensor contains the trusted logic which:

1. Runs Bittensor's [consensus mechanism](./docs/consensus.md);
2. Advertises neuron information, IPs, etc., and
3. Facilitates value transfer via TAO.

## System Requirements

* The binaries in ./bin/release are x86_64 binaries to be used with the Linux kernel.
* Subtensor needs ~286 MiB to run.
*	Supported Architectures:
	-	Linux: x86_64
	-	MacOS: x86_64 and ARM64 (M series Macs)
* OSs other than Linux and MacOS are currently not supported.


## Architectures
Subtensor support the following architectures:

## Linux x86_64
Requirements:
* Linux kernel 2.6.32+,
* glibc 2.11+
A fresh FRAME-based [Substrate](https://www.substrate.io/) node, ready for hacking :rocket:

## MacOS x86_64 & arm64 (Apple Silicon)
Requirements:
*	macOS 10.7+ (Lion+) for x86_64
*	macOS 11+ (Big Sur+) for Apple Silicon (M1, M2, and later) with arm64 architecture support.

## Network requirements
* Subtensor needs access to the public internet
* Subtensor runs on ipv4
* Subtensor listens on the following ports:
  1) 9944 - Websocket. This port is used by bittensor. It only accepts connections from localhost. Make sure this port is firewalled off from the public domain.
  2) 9933 - RPC. This port is opened, but not used.
  3) 30333 - p2p socket. This port accepts connections from other subtensor nodes. Make sure your firewall(s) allow incoming traffic to this port.
* It is assumed your default outgoing traffic policy is ACCEPT. If not, make sure outbound traffic to port 30333 is allowed.

---

## For Subnet Development

If you are developing and testing subnet incentive mechanism, you will need to run a local subtensor node. Follow the detailed step-by-step instructions provided in the [**Subtensor Nodes** section in Bittensor Developer Documentation](https://docs.bittensor.com/subtensor-nodes).

### Lite node vs Archive node

For an explanation of lite node, archive node and how you can run your local subtensor node in these modes, see [Lite node vs archive node](https://docs.bittensor.com/subtensor-nodes#lite-node-vs-archive-node) section on [Bittensor Developer Docs](https://docs.bittensor.com/).

---

## For Subtensor Development

### Installation
First, complete the [basic Rust setup instructions](./docs/rust-setup.md).

**Build and Run**

Use Rust's native `cargo` command to build and launch the template node:

```sh
cargo run --release -- --dev
```

**Build only**

The above `cargo run` command will perform an initial build and launch the node. Use the following command to build the node
without launching it:

```sh
cargo build --release
```

<!--

/** When I ran "cargo doc" it gave me a bunch of errors. And when I did "cargo doc --open" it gave same bunch of errors, and did not open. Also, I don't think the binary is "subtensor". It is "node-subtensor". We should uncomment this section after testing and validating and fixing this section.
*/

### Embedded Docs

Once the project has been built, the following command can be used to explore all parameters and
subcommands:

```sh
./target/release/subtensor -h
```
-->

## Other ways to launch the node

The above `cargo run` command will launch a temporary node and its state will be discarded after
you terminate the process. After the project has been built, there are other ways to launch the
node.

### Single-Node Development Chain

This command will start the single-node development chain with non-persistent state:

```bash
./target/release/subtensor --dev
```

Purge the development chain's state:

```bash
./target/release/subtensor purge-chain --dev
```

Start the development chain with detailed logging:

```bash
RUST_BACKTRACE=1 ./target/release/subtensor-ldebug --dev
```

Running debug with logs.
```bash
SKIP_WASM_BUILD=1 RUST_LOG=runtime=debug -- --nocapture
```

Running individual tests
```bash
SKIP_WASM_BUILD=1 \
  RUST_LOG=runtime=debug \
  cargo test <your test name> \
  -- --nocapture --color always
```

<details>
  <summary>testing `tests/` tips</summary>

  **`<package-name>`**
  Available members are found within the project root [`./cargo.toml`](./cargo.toml) file, each
  point to a sub-directory containing a `cargo.toml` file with a `name` defined.  for example,
  [`node/cargo.toml`](./node/cargo.toml) has a name of `node-subtensor`


  **`<test-name>`**
  Available tests are often found within either a `tests/` sub-directory or within the relevant
  `src/` file.  for example [`./node/tests/chain_spec.rs`](./node/tests/chain_spec.rs) has a test
  named `chain_spec`

  **example**
  All together we can run all tests in `chain_spec` file from `node-subtensor` project via

  ```bash
  skip_wasm_build=1 \
    rust_log=runtime=debug \
    cargo test \
    --package node-subtensor \
    --test chain_spec \
    -- --color always --nocapture
  ```
</details>


Running code coverage
```bash
bash scripts/code-coverage.sh
```

> Note: They above requires `cargo-tarpaulin` is installed to the host, eg. `cargo install cargo-tarpaulin`
> Development chain means that the state of our chain will be in a tmp folder while the nodes are
> running. Also, **alice** account will be authority and sudo account as declared in the
> [genesis state](https://github.com/substrate-developer-hub/substrate-node-template/blob/main/node/src/chain_spec.rs#L49).
> At the same time the following accounts will be pre-funded:
> - Alice
> - Bob
> - Alice//stash
> - Bob//stash

If we want to maintain the chain state between runs, a base path must be added
so the db can be stored in the provided folder instead of a temporal one. We could use this folder
to store different chain databases, as a different folder will be created per different chain that
is ran. The following commands show how to use a newly created folder as our db base path:

```bash
# Create a folder to use as the db base path
mkdir my-chain-state

# Use of that folder to store the chain state
./target/release/node-template --dev --base-path ./my-chain-state/

# Check the folder structure created inside the base path after running the chain
ls ./my-chain-state
#> chains
ls ./my-chain-state/chains/
#> dev
ls ./my-chain-state/chains/dev
#> db keystore network
```

**Connect with Polkadot-JS Apps Front-end**

Once the node template is running locally, you can connect it with **Polkadot-JS Apps** front-end
to interact with your chain. [Click
here](https://polkadot.js.org/apps/#/explorer?rpc=ws://localhost:9944) connecting the Apps to your
local node template.

### Multi-Node Local Testnet

If you want to see the multi-node consensus algorithm in action, refer to our
[Simulate a network tutorial](https://docs.substrate.io/tutorials/build-a-blockchain/simulate-network/).

## Template Structure

A Substrate project such as this consists of a number of components that are spread across a few
directories.

### Node Capabilities

A blockchain node is an application that allows users to participate in a blockchain network.
Substrate-based blockchain nodes expose a number of capabilities:

- Networking: Substrate nodes use the [`libp2p`](https://libp2p.io/) networking stack to allow the
  nodes in the network to communicate with one another.
- Consensus: Blockchains must have a way to come to
  [consensus](https://docs.substrate.io/main-docs/fundamentals/consensus/) on the state of the
  network. Substrate makes it possible to supply custom consensus engines and also ships with
  several consensus mechanisms that have been built on top of
  [Web3 Foundation research](https://research.web3.foundation/Polkadot/protocols/NPoS/Overview).
- RPC Server: A remote procedure call (RPC) server is used to interact with Substrate nodes.

**Directory structure**

There are several files in the [`node`](./node/) directory. Make a note of the following important files:

- [`chain_spec.rs`](./node/src/chain_spec.rs): A
  [chain specification](https://docs.substrate.io/main-docs/build/chain-spec/) is a
  source code file that defines a Substrate chain's initial (genesis) state. Chain specifications
  are useful for development and testing, and critical when architecting the launch of a
  production chain. Take note of the `development_config` and `testnet_genesis` functions, which
  are used to define the genesis state for the local development chain configuration. These
  functions identify some
  [well-known accounts](https://docs.substrate.io/reference/command-line-tools/subkey/)
  and use them to configure the blockchain's initial state.
- [`service.rs`](./node/src/service.rs): This file defines the node implementation. Take note of
  the libraries that this file imports and the names of the functions it invokes. In particular,
  there are references to consensus-related topics, such as the
  [block finalization and forks](https://docs.substrate.io/main-docs/fundamentals/consensus/#finalization-and-forks)
  and other [consensus mechanisms](https://docs.substrate.io/main-docs/fundamentals/consensus/#default-consensus-models)
  such as Aura for block authoring and GRANDPA for finality.

### CLI help

After the node has been [built](#build), refer to the embedded documentation to learn more about the
capabilities and configuration parameters that it exposes:

```shell
./target/release/node-subtensor --help
```

### Runtime

In Substrate, the terms
"runtime" and "state transition function"
are analogous - they refer to the core logic of the blockchain that is responsible for validating
blocks and executing the state changes they define. The Substrate project in this repository uses
[FRAME](https://docs.substrate.io/main-docs/fundamentals/runtime-intro/#frame) to construct a
blockchain runtime. FRAME allows runtime developers to declare domain-specific logic in modules
called "pallets". At the heart of FRAME is a helpful
[macro language](https://docs.polkadot.com/develop/parachains/customize-parachain/overview/#pallet-structure) that makes it easy to
create pallets and flexibly compose them to create blockchains that can address
[a variety of needs](https://substrate.io/ecosystem/projects/).

Review the [FRAME runtime implementation](./runtime/src/lib.rs) included in this template and note
the following:

- This file configures several pallets to include in the runtime. Each pallet configuration is
  defined by a code block that begins with `impl $PALLET_NAME::Config for Runtime`.
- The pallets are composed into a single runtime by way of the
  [`construct_runtime!`](https://crates.parity.io/frame_support/macro.construct_runtime.html)
  macro, which is part of the core
  FRAME Support [system](https://docs.substrate.io/reference/frame-pallets/#system-pallets) library.

### Pallets

The runtime in this project is constructed using many FRAME pallets that ship with the
[core Substrate repository](https://github.com/paritytech/substrate/tree/master/frame) and a
template pallet that is [defined in the `pallets`](./pallets/template/src/lib.rs) directory.

A FRAME pallet is compromised of a number of blockchain primitives:

- Storage: FRAME defines a rich set of powerful
  [storage abstractions](https://docs.substrate.io/main-docs/build/runtime-storage/) that makes
  it easy to use Substrate's efficient key-value database to manage the evolving state of a
  blockchain.
- Dispatchables: FRAME pallets define special types of functions that can be invoked (dispatched)
  from outside of the runtime in order to update its state.
- Events: Substrate uses [events and errors](https://docs.substrate.io/main-docs/build/events-errors/)
  to notify users of important changes in the runtime.
- Errors: When a dispatchable fails, it returns an error.
- Config: The `Config` configuration interface is used to define the types and parameters upon
  which a FRAME pallet depends.

<!--
### Run in Docker

First, install [Docker](https://docs.docker.com/get-docker/) and
[Docker Compose](https://docs.docker.com/compose/install/).

Then run the following command to start a single node development chain.

```bash
./scripts/docker_run.sh
```

This command will firstly compile your code, and then start a local development network. You can
also replace the default command
(`cargo build --release && ./target/release/node-template --dev --ws-external`)
by appending your own. A few useful ones are as follow.

```bash
# Run Substrate node without re-compiling
./scripts/docker_run.sh ./target/release/node-template --dev --ws-external

# Purge the local dev chain
./scripts/docker_run.sh ./target/release/node-template purge-chain --dev

# Check whether the code is compilable
./scripts/docker_run.sh cargo check
```
-->

## License
The MIT License (MIT)
Copyright © 2021 Yuma Rao

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.


## Acknowledgments
**parralax**
