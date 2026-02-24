# Minimal image: uv + pymdurs, volume for examples (Debian).
# Build:  docker build -t pymdurs-examples .
# Run:    docker run --rm -it -v "$(pwd)/examples:/app/examples" pymdurs-examples sh
# Example: docker run --rm -it -v "$(pwd)/examples:/app/examples" pymdurs-examples python examples/building_basic.py

FROM ghcr.io/astral-sh/uv:python3.12-bookworm-slim

WORKDIR /app

# Installer uniquement les libs runtime GDAL/GEOS/PROJ
# (pas les -dev car on n'a pas besoin de compiler)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libgdal-dev \
    libgeos-dev \
    libproj-dev \
    && rm -rf /var/lib/apt/lists/*

# Installer pymdurs — tous les wheels sont dispo sur Debian x86_64
RUN uv pip install --system pymdurs

VOLUME /app/examples

CMD ["sh"]