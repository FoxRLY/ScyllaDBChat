FROM rust:latest as build
RUN USER=root cargo new chat
WORKDIR /chat
RUN echo $(pwd)
RUN echo $(ls)
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --release
RUN rm src/*.rs
COPY ./src ./src
RUN rm ./target/release/chat*
RUN cargo build --release

FROM rust:latest
COPY --from=build /chat/target/release/chat .
CMD ["./chat"]
