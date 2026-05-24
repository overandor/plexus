# Deployment Guide

## Prerequisites

- Rust stable
- Cargo

## Local Development

```bash
cargo build --release
cargo run
```

## Docker Deployment

```bash
docker build -t plexus .
docker run -p 8000:8000 plexus
```

## Kubernetes

```bash
kubectl apply -f k8s/
```
