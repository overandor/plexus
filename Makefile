.PHONY: build run fmt lint clean

build:
	cargo build --release

run:
	cargo run

fmt:
	cargo fmt

lint:
	cargo clippy -- -D warnings

clean:
	cargo clean
