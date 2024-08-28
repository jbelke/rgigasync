# Use a newer Debian base image with a more recent GLIBC version
FROM rust:1.80-slim-bookworm as builder

WORKDIR /rgigasync

# Copy the source code and build
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Use the same base image for the final container
FROM debian:bookworm-slim

WORKDIR /rgigasync

COPY --from=builder /rgigasync/target/release/rgigasync /usr/local/bin/rgigasync

# Command to run your application
CMD ["rgigasync"]
