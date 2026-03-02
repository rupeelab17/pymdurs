"""
Example: Streamlines from wind velocity file (wind_uv.dat).

Reads the velocity file produced by the Röckle wind field when the mass-consistent
solver is used (wind_uv.dat: x, y, z, u, v per pixel). Plots streamlines in world
coordinates (east, north) using matplotlib.streamplot.

Usage:
   python examples/plot_wind_streamlines.py [path_to_wind_uv.dat]
   python examples/plot_wind_streamlines.py output/wind_10dir/dir_000

If a directory is given, looks for wind_uv.dat inside it.
Output: streamlines.png in the same directory as the .dat file (or current dir).
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import numpy as np

try:
    import matplotlib.pyplot as plt
except ImportError:
    plt = None


def load_wind_uv_dat(path: Path) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, int, int, float]:
    """Load wind_uv.dat. Returns (x_2d, y_2d, u, v, width, height, output_height)."""
    with path.open() as f:
        header = f.readline().strip()
        if not header.startswith("#"):
            raise ValueError(f"Expected header starting with '#', got {header!r}")
        parts = header[1:].strip().split()
        if len(parts) != 3:
            raise ValueError(f"Expected '# width height output_height', got {header!r}")
        nx, ny, output_height = int(parts[0]), int(parts[1]), float(parts[2])
    data = np.loadtxt(path, comments="#")
    if data.size != nx * ny * 5:
        raise ValueError(f"File has {data.size // 5} rows, expected {nx * ny}")
    # row-major: first nx rows are row 0, next nx are row 1, ...
    data = data.reshape(ny, nx, 5)
    x_2d = data[:, :, 0]
    y_2d = data[:, :, 1]
    u = data[:, :, 3]
    v = data[:, :, 4]
    return x_2d, y_2d, u, v, nx, ny, output_height


def plot_streamlines(
    dat_path: Path,
    out_path: Path | None = None,
    density: float = 1.5,
    linewidth_scale: float = 0.5,
    color_speed: bool = True,
    background_speed: bool = True,
) -> None:
    """Load wind_uv.dat and plot streamlines; optionally save to out_path."""
    if plt is None:
        raise ImportError("matplotlib is required: pip install matplotlib")

    x_2d, y_2d, u, v, nx, ny, _ = load_wind_uv_dat(dat_path)

    # 1D coordinates for streamplot (columns = x, rows = y)
    x_1d = x_2d[0, :]
    y_1d = y_2d[:, 0]

    speed = np.hypot(u, v)
    u_masked = np.where(speed > 1e-6, u, 0.0)
    v_masked = np.where(speed > 1e-6, v, 0.0)

    fig, ax = plt.subplots(1, 1, figsize=(10, 8))

    if background_speed:
        im = ax.pcolormesh(x_1d, y_1d, speed, shading="auto", cmap="Blues", alpha=0.6)
        plt.colorbar(im, ax=ax, label="Wind speed (m/s)")

    if color_speed:
        strm = ax.streamplot(
            x_1d,
            y_1d,
            u_masked,
            v_masked,
            color=speed,
            cmap="viridis",
            linewidth=linewidth_scale * np.clip(speed, 0, None) + 0.3,
            density=density,
            arrowsize=1.2,
        )
        plt.colorbar(strm.lines, ax=ax, label="Speed (m/s)")
    else:
        ax.streamplot(
            x_1d,
            y_1d,
            u_masked,
            v_masked,
            linewidth=linewidth_scale * np.clip(speed, 0, None) + 0.3,
            density=density,
            arrowsize=1.2,
            color="k",
        )

    ax.set_xlabel("X (m)")
    ax.set_ylabel("Y (m)")
    ax.set_aspect("equal")
    ax.set_title(f"Wind streamlines — {dat_path.name}")
    plt.tight_layout()

    if out_path is not None:
        fig.savefig(out_path, dpi=150)
        print(f"Saved {out_path}")
    else:
        plt.show()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Plot wind streamlines from wind_uv.dat (Röckle solver output)."
    )
    parser.add_argument(
        "path",
        nargs="?",
        default="output/wind_10dir/dir_000/wind_uv.dat",
        help="Path to wind_uv.dat or directory containing it",
    )
    parser.add_argument(
        "-o", "--output",
        help="Output PNG path (default: streamlines.png next to the .dat file)",
    )
    parser.add_argument("--density", type=float, default=1.5, help="Streamline density")
    parser.add_argument("--no-color", action="store_true", help="Do not color streamlines by speed")
    parser.add_argument("--no-background", action="store_true", help="Do not show speed as background")
    args = parser.parse_args()

    p = Path(args.path)
    if p.is_dir():
        dat_path = p / "wind_uv.dat"
    else:
        dat_path = p

    if not dat_path.is_file():
        print(f"Error: {dat_path} not found.", file=sys.stderr)
        return 1

    out_path = Path(args.output) if args.output else dat_path.parent / "streamlines.png"

    plot_streamlines(
        dat_path,
        out_path=out_path,
        density=args.density,
        color_speed=not args.no_color,
        background_speed=not args.no_background,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
