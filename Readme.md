# rgigasync

`rgigasync` is a high-performance, Rust-based command-line tool that enhances the capabilities of `rsync` by enabling efficient mirroring of large directory trees. It is designed to overcome the limitations of `rsync` when dealing with massive file sets, providing speed improvements and greater resilience in handling large-scale file synchronization tasks.

## Why Use `rgigasync`?

When synchronizing vast amounts of data across networks or between storage systems, `rsync` can struggle with performance issues, particularly in scenarios involving:

- **Large Numbers of Files**: `rsync`'s memory usage grows with the number of files, which can lead to excessive memory consumption and slowdowns.
- **Network Instability**: If a network connection fails during a large sync operation, `rsync` needs to restart the entire process, leading to inefficiencies and potential data transfer interruptions.
- **Complex Directory Structures**: Deep and complex directory structures can cause `rsync` to perform suboptimally, especially when it attempts to determine the full set of changes before starting the transfer.

`rgigasync` addresses these challenges by breaking down the synchronization process into manageable batches, allowing for:

- **Optimized Memory Usage**: By processing files in smaller batches, `rgigasync` reduces the memory footprint, enabling `rsync` to handle large directories without consuming excessive system resources.
- **Increased Resilience**: With its built-in retry mechanism, `rgigasync` can automatically retry failed `rsync` operations, ensuring that data synchronization continues even in the face of network instability.
- **Faster Synchronizations**: By parallelizing and batching file transfers, `rgigasync` can significantly speed up the synchronization process, especially when dealing with large numbers of small files.

## Features

- **Batch Processing**: Files are processed in batches to prevent `rsync` from using too much memory, making it suitable for syncing millions of files.
- **Retry Mechanism**: Automatically retries `rsync` operations up to 5 times in case of failure, improving reliability in unstable network environments.
- **Customizable**: Allows passing custom `rsync` options and specifying batch sizes, offering flexibility for different use cases.
- **Speed Optimization**: Designed to maximize the efficiency of `rsync` by reducing overhead and improving throughput, particularly in large-scale operations.

## Requirements

- **Rust**: Programming language and Cargo (Rust's package manager) for building the project.
- **rsync**: Installed on your system, as `rgigasync` leverages `rsync` for file synchronization.

## Installation: Adding `rgigasync` to Your PATH

To make `rgigasync` available globally in your terminal, you can copy the compiled binary to a directory that's included in your system's `PATH`, or you can add the binary's location to your `PATH`.

### Option 1: Copy the Binary to `/usr/local/bin` (Recommended for macOS Users)

This option copies the `rgigasync` binary to `/usr/local/bin`, a common directory in the `PATH`:

1. **Build the Project** (if you haven't already):
    ```bash
    cargo build --release
    ```

2. **Create `/usr/local/bin` (if it doesn't exist)**:
    On macOS, if the `/usr/local/bin` directory does not exist, you can create it with:
    ```bash
    sudo mkdir -p /usr/local/bin
    ```

3. **Set Permissions**:
    Ensure the directory is writable by your user account:
    ```bash
    sudo chown -R $(whoami) /usr/local/bin
    ```

4. **Copy the Binary**:
    ```bash
    cp ./target/release/rgigasync /usr/local/bin/
    ```

5. **Verify Installation**:
    After copying, you can verify the installation by running:
    ```bash
    rgigasync --version
    ```
    If the installation was successful, this should display the version of the tool.

### Option 2: Add the Binary to a Custom `bin` Directory in Your Home Directory

If you prefer not to use `/usr/local/bin`, you can create a `bin` directory in your home directory and add it to your `PATH`:

1. **Create a Custom Bin Directory**:
   ```bash
   mkdir -p ~/bin

### Common Examples:

# Common Usage Examples

- ** Example 1: Basic Synchronization with Verbose Output
 `rgigasync -- "-av" /Volumes/SrcDir/ /Users/userName/DestDir/`

- ** Example 2: Synchronization with Progress and Ignoring Existing Files
 `rgigasync -- "-av --ignore-existing --info=progress2" /Volumes/SrcDir/ /Users/userName/DestDir/`

- ** Example 3: Specifying a Custom Batch Size
 `rgigasync -- "-av --ignore-existing --info=progress2" /Volumes/SrcDir/ /Users/userName/DestDir/ 512`

- ** Example 4: Excluding Specific File Types
 `rgigasync -- "-av --exclude='*.tmp' --exclude='*.log'" /Volumes/SrcDir/ /Users/userName/DestDir/`

- ** Example 5: Synchronization Over SSH
 `rgigasync -- "-avz -e ssh" /Volumes/SrcDir/ user@remote-server:/home/user/DestDir/`

- ** Example 6: Deleting Files at Destination That Are Not Present at Source
  `rgigasync -- "-av --delete" /Volumes/SrcDir/ /Users/userName/DestDir/`

- ** Example 7: Limiting Bandwidth Usage
  `rgigasync -- "-av --bwlimit=10240" /Volumes/SrcDir/ /Users/userName/DestDir/`

- ** Example 8: Dry Run to Preview Changes
  `rgigasync -- "-av --dry-run" /Volumes/SrcDir/ /Users/userName/DestDir/`

