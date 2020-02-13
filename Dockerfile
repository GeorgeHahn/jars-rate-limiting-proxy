# WARNING: I haven't actually tried this. I think it will work, but I'm developing on my Windows
# PC at home and haven't done a docker build.

# derived from https://alexbrand.dev/post/how-to-package-rust-applications-into-minimal-docker-containers/
# this is okay, but consider switching to https://github.com/denzp/cargo-wharf
FROM rust:1.41.0 AS build
WORKDIR /usr/src

# Download the target for static linking.
RUN rustup target add x86_64-unknown-linux-musl

# Workaround: cargo doesn't currently offer a way to build dependencies only.
# The solution is to build a dummy project first to cache the dep build.
# More info at: https://github.com/rust-lang/cargo/issues/2644
RUN USER=root cargo new jars
WORKDIR /usr/src/jars
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release

# Copy the source and build the application.
COPY src ./src
RUN cargo install --target x86_64-unknown-linux-musl --path .

# Copy the statically-linked binary into a new container.
FROM scratch
COPY --from=build /usr/local/cargo/bin/jars /bin/jars
USER 1000
CMD ["/bin/jars"]