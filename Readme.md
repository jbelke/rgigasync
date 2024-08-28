## Installation: Adding `rgigasync` to Your PATH

To make `rgigasync` available globally in your terminal, you can copy the compiled binary to a directory that's included in your system's `PATH`, or you can add the directory containing the binary to your `PATH`.

### Option 1: Copy the Binary to `/usr/local/bin`

This option copies the `rgigasync` binary to `/usr/local/bin`, a common directory in the `PATH`:

1. **Build the Project** (if you haven't already):
    ```bash
    cargo build --release
    ```

2. **Copy the Binary**:
    ```bash
    sudo cp ./target/release/rgigasync /usr/local/bin/
    ```

3. **Verify Installation**:
    After copying, you can verify the installation by running:
    ```bash
    rgigasync --version
    ```
    If the installation was successful, this should display the version of the tool.

### Option 2: Add the Binary to Your `PATH`

If you prefer not to copy the binary, you can add the directory containing the binary to your `PATH`:

1. **Build the Project** (if you haven't already):
    ```bash
    cargo build --release
    ```

2. **Add the Directory to Your PATH**:
    ```bash
    export PATH="$PATH:/path/to/rgigasync/target/release"
    ```
    Replace `/path/to/rgigasync/target/release` with the actual path to the directory where the binary is located.

3. **Make the Change Permanent**:
    To make this change permanent, add the above `export` command to your shell configuration file (e.g., `~/.bashrc`, `~/.zshrc`, etc.):
    ```bash
    echo 'export PATH="$PATH:/path/to/rgigasync/target/release"' >> ~/.bashrc
    # or for zsh users
    echo 'export PATH="$PATH:/path/to/rgigasync/target/release"' >> ~/.zshrc
    ```

4. **Source the Configuration File**:
    After editing the configuration file, apply the changes:
    ```bash
    source ~/.bashrc
    # or for zsh users
    source ~/.zshrc
    ```

5. **Verify Installation**:
    You can now verify that `rgigasync` is accessible from anywhere in your terminal:
    ```bash
    rgigasync --version
    ```

With these steps, you should be able to run `rgigasync` from anywhere in your terminal without needing to specify the full path to the binary.

