# rgigasync

`rgigasync` is a Rust-based command-line tool that facilitates the use of `rsync` to mirror large directory trees. This tool is particularly useful for handling large sets of files where `rsync` alone may struggle due to memory constraints or network instability.

## Features

- **Batch Processing**: Files are processed in batches to prevent `rsync` from using too much memory.
- **Retry Mechanism**: Automatically retries `rsync` operations up to 5 times in case of failure.
- **Customizable**: Allows passing custom `rsync` options and specifying batch sizes.

## Requirements

- Rust programming language and Cargo (Rust's package manager).
- `rsync` installed on your system.

## Installation

To install `rgigasync`, you need to have Rust installed on your system. Follow the steps below:

1. Clone the repository (if applicable):
    ```bash
    git clone https://github.com/jbelke/rgigasync.git
    cd rgigasync
    ```

2. Build the project:
    ```bash
    cargo build --release
    ```

3. The compiled binary will be located in the `target/release/` directory:
    ```bash
    ./target/release/rgigasync
    ```

## Usage

### Syntax

```bash
rgigasync <rsync-options> <src-dir> <target-dir> [run-size-mb]

