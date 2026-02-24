# Minimal image: uv + pymdurs, volume for examples (Alpine).
# Build:  docker build -t pymdurs-examples .
# Run:    docker run --rm -it -v "$(pwd)/examples:/app/examples" pymdurs-examples sh
# Example: docker run --rm -it -v "$(pwd)/examples:/app/examples" pymdurs-examples python examples/building_basic.py
FROM ghcr.io/astral-sh/uv:python3.12-alpine

WORKDIR /app

RUN uv pip install --system pymdurs

VOLUME /app/examples

CMD ["sh"]
