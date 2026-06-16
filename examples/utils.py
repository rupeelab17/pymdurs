"""Raster preview utilities for SOLWEIG outputs (PNG, GIF, legends, colormaps)."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import rasterio
from PIL import Image, ImageDraw, ImageFont

PHI = (1 + 5**0.5) / 2  # nombre d'or
DAYTIME_HOUR_START = 7
DAYTIME_HOUR_END = 19
_LEGEND_BAR_HEIGHT = 22
_LEGEND_TEXT_HEIGHT = 36
_LEGEND_MARGIN = 8


def read_raster_array(path: Path) -> np.ndarray:
    with rasterio.open(path) as src:
        data = src.read(1).astype(np.float64)
        nodata = src.nodata
        if nodata is not None:
            data = np.where(data == nodata, np.nan, data)
    return data


def parse_raster_hour(path: Path) -> int | None:
    """Extract hour from SOLWEIG raster names like ``tmrt_20250701_1200.tif``."""
    time_part = path.stem.rsplit("_", 1)[-1]
    if len(time_part) == 4 and time_part.isdigit():
        return int(time_part[:2])
    return None


def compute_daytime_value_range(
    paths: list[Path],
    hour_start: int = DAYTIME_HOUR_START,
    hour_end: int = DAYTIME_HOUR_END,
) -> tuple[float, float] | None:
    """Return min/max of valid pixels across rasters between ``hour_start`` and ``hour_end``."""
    daytime_paths = [
        path
        for path in paths
        if (hour := parse_raster_hour(path)) is not None and hour_start <= hour <= hour_end
    ]
    if not daytime_paths:
        return None

    chunks: list[np.ndarray] = []
    for path in daytime_paths:
        data = read_raster_array(path)
        valid = data[np.isfinite(data)]
        if valid.size:
            chunks.append(valid)

    if not chunks:
        return None

    pooled = np.concatenate(chunks)
    vmin, vmax = float(np.min(pooled)), float(np.max(pooled))
    if vmax <= vmin:
        vmax = vmin + 1.0
    return vmin, vmax


def compute_value_range(
    paths: list[Path],
    hour_start: int = DAYTIME_HOUR_START,
    hour_end: int = DAYTIME_HOUR_END,
) -> tuple[float, float] | None:
    """Daytime min/max for legends; fall back to all timesteps if needed."""
    value_range = compute_daytime_value_range(paths, hour_start=hour_start, hour_end=hour_end)
    if value_range is not None:
        return value_range

    chunks: list[np.ndarray] = []
    for path in paths:
        if path.suffix.lower() not in {".tif", ".tiff"}:
            continue
        data = read_raster_array(path)
        valid = data[np.isfinite(data)]
        if valid.size:
            chunks.append(valid)
    if not chunks:
        return None
    pooled = np.concatenate(chunks)
    vmin, vmax = float(np.min(pooled)), float(np.max(pooled))
    if vmax <= vmin:
        vmax = vmin + 1.0
    return vmin, vmax


def format_scale_caption(
    variable: str,
    vmin: float,
    vmax: float,
    hour_start: int,
    hour_end: int,
) -> str:
    return (
        f"Echelle fixe {hour_start}h-{hour_end}h: "
        f"min={vmin:.2f}, max={vmax:.2f} ({variable})"
    )


def append_colorbar_legend(
    img: Image.Image,
    *,
    cmap: str,
    vmin: float,
    vmax: float,
    caption: str,
    unit: str = "°C",
) -> Image.Image:
    """Append a horizontal colorbar and scale caption below the preview image."""
    import matplotlib.pyplot as plt

    if img.mode == "RGBA":
        base = Image.new("RGB", img.size, (255, 255, 255))
        base.paste(img, mask=img.split()[3])
        img = base
    elif img.mode != "RGB":
        img = img.convert("RGB")

    width, height = img.size
    footer_h = _LEGEND_BAR_HEIGHT + _LEGEND_TEXT_HEIGHT + 2 * _LEGEND_MARGIN
    gradient = np.linspace(0, 1, width, dtype=np.float64).reshape(1, -1)
    bar_rgb = (plt.colormaps[cmap](gradient)[..., :3] * 255).astype(np.uint8)
    bar_img = Image.fromarray(bar_rgb).resize(
        (width, _LEGEND_BAR_HEIGHT),
        Image.Resampling.NEAREST,
    )

    out = Image.new("RGB", (width, height + footer_h), (255, 255, 255))
    out.paste(img, (0, 0))
    bar_y = height + _LEGEND_MARGIN
    out.paste(bar_img, (0, bar_y))

    draw = ImageDraw.Draw(out)
    font = ImageFont.load_default()
    unit_suffix = f" {unit}" if unit else ""
    label_y = bar_y + _LEGEND_BAR_HEIGHT + 4
    draw.text((4, label_y), f"{vmin:.2f}{unit_suffix}", fill=(0, 0, 0), font=font)
    max_label = f"{vmax:.2f}{unit_suffix}"
    max_w = draw.textlength(max_label, font=font)
    draw.text((width - max_w - 4, label_y), max_label, fill=(0, 0, 0), font=font)
    caption_w = draw.textlength(caption, font=font)
    draw.text(((width - caption_w) / 2, label_y + 14), caption, fill=(0, 0, 0), font=font)
    return out


def append_shadow_legend(img: Image.Image, variable: str) -> Image.Image:
    """Append a grayscale legend for shadow previews."""
    caption = f"Ombre (0 = sous l'ombrage, 1 = au soleil) ({variable})"
    return append_colorbar_legend(
        img,
        cmap="gray",
        vmin=0.0,
        vmax=1.0,
        caption=caption,
        unit="",
    )


def raster_to_frame(
    path: Path,
    mode: str = "continuous",
    cmap: str | None = None,
    vmin: float | None = None,
    vmax: float | None = None,
) -> Image.Image:
    """Convert a GeoTIFF band to a PIL frame for GIF/PNG encoding."""
    data = read_raster_array(path)

    if mode == "shadow":
        arr = np.nan_to_num(data, nan=0.0)
        scaled = (np.clip(arr, 0, 1) * 255).astype(np.uint8)
        return Image.fromarray(scaled, mode="L").convert("P", palette=Image.ADAPTIVE)

    valid = data[np.isfinite(data)]
    mask = ~np.isfinite(data)
    if valid.size == 0:
        norm = np.zeros(data.shape, dtype=np.float64)
    elif vmin is not None and vmax is not None:
        norm = ((np.nan_to_num(data, nan=vmin) - vmin) / (vmax - vmin)).clip(0, 1)
    else:
        pmin, pmax = np.percentile(valid, [2, 98])
        if pmax <= pmin:
            pmax = pmin + 1.0
        norm = ((np.nan_to_num(data, nan=pmin) - pmin) / (pmax - pmin)).clip(0, 1)

    if cmap:
        import matplotlib.pyplot as plt

        rgba = plt.colormaps[cmap](norm)
        rgba[mask] = (0, 0, 0, 0)
        return Image.fromarray((rgba * 255).astype(np.uint8), mode="RGBA")

    scaled = (norm * 255).astype(np.uint8)
    return Image.fromarray(scaled, mode="L").convert("P", palette=Image.ADAPTIVE)


def find_raster_paths(folder: Path, pattern: str) -> list[Path]:
    """Resolve raster paths; fall back to legacy PNG previews if no GeoTIFF match."""
    paths = sorted(folder.glob(pattern))
    if paths:
        return paths
    prefix = pattern.split("*", 1)[0]
    for fallback in (f"{prefix}*.preview.png", f"{prefix}*.png"):
        paths = sorted(folder.glob(fallback))
        if paths:
            return paths
    return []


def prepare_png_frame(
    img: Image.Image,
    scale: float = 2.0,
    golden_ratio: bool = True,
) -> Image.Image:
    """Upscale a preview image and optionally pad it to a golden-ratio canvas."""
    w = max(1, round(img.width * scale))
    h = max(1, round(img.height * scale))
    img = img.resize((w, h), Image.Resampling.LANCZOS)

    if not golden_ratio:
        return img

    if w >= h:
        canvas_w = max(w, round(h * PHI))
        canvas_h = max(h, round(canvas_w / PHI))
    else:
        canvas_h = max(h, round(w * PHI))
        canvas_w = max(w, round(canvas_h / PHI))

    if canvas_w == w and canvas_h == h:
        return img

    canvas = Image.new("RGBA", (canvas_w, canvas_h), (0, 0, 0, 0))
    canvas.paste(img, ((canvas_w - w) // 2, (canvas_h - h) // 2))
    return canvas


def frame_for_gif(img: Image.Image) -> Image.Image:
    """Convert a raster frame to a GIF-compatible palette image."""
    if img.mode == "RGBA":
        background = Image.new("RGB", img.size, (255, 255, 255))
        background.paste(img, mask=img.split()[3])
        img = background
    return img.convert("P", palette=Image.ADAPTIVE)


def _rgba_to_rgb(img: Image.Image) -> Image.Image:
    if img.mode == "RGBA":
        background = Image.new("RGB", img.size, (255, 255, 255))
        background.paste(img, mask=img.split()[3])
        return background
    if img.mode != "RGB":
        return img.convert("RGB")
    return img


def _apply_preview_legend(
    frame: Image.Image,
    *,
    mode: str,
    cmap: str | None,
    value_range: tuple[float, float] | None,
    legend_caption: str | None,
    variable: str,
    unit: str | None,
) -> Image.Image:
    if mode == "shadow":
        return append_shadow_legend(frame, variable)
    if cmap and value_range is not None and legend_caption is not None:
        return append_colorbar_legend(
            frame,
            cmap=cmap,
            vmin=value_range[0],
            vmax=value_range[1],
            caption=legend_caption,
            unit=unit or "°C",
        )
    return frame


def preview_rasters_to_gif(
    folder: str | Path,
    pattern: str = "shadow_*.tif",
    out_path: str | Path | None = None,
    duration_ms: int = 500,
    loop: int = 0,
    mode: str = "continuous",
    cmap: str | None = None,
    hour_start: int = DAYTIME_HOUR_START,
    hour_end: int = DAYTIME_HOUR_END,
    variable: str | None = None,
    unit: str | None = "°C",
) -> Path | None:
    """Create an animated GIF from SOLWEIG GeoTIFFs (or legacy preview PNGs)."""
    folder = Path(folder)
    variable = variable or folder.name
    out_path = Path(out_path) if out_path else folder / "preview.gif"
    paths = find_raster_paths(folder, pattern)
    if not paths:
        print(f"⚠️  No files found for GIF: {folder / pattern}")
        return None

    value_range = compute_value_range(paths, hour_start=hour_start, hour_end=hour_end) if cmap else None
    legend_caption = None
    if value_range is not None:
        vmin, vmax = value_range
        legend_caption = format_scale_caption(variable, vmin, vmax, hour_start, hour_end)
        print(f"  {legend_caption}")

    frames: list[Image.Image] = []
    for path in paths:
        if path.suffix.lower() in {".png", ".jpg", ".jpeg"}:
            frame = Image.open(path).convert("RGB")
        else:
            frame_kwargs: dict = {"mode": mode, "cmap": cmap}
            if value_range is not None:
                frame_kwargs["vmin"], frame_kwargs["vmax"] = value_range
            frame = _rgba_to_rgb(raster_to_frame(path, **frame_kwargs))

        frame = _apply_preview_legend(
            frame,
            mode=mode,
            cmap=cmap,
            value_range=value_range,
            legend_caption=legend_caption,
            variable=variable,
            unit=unit,
        )
        frames.append(frame_for_gif(frame))

    frames[0].save(
        out_path,
        save_all=True,
        append_images=frames[1:],
        duration=duration_ms,
        loop=loop,
    )
    return out_path


def export_rasters_to_pngs(
    folder: str | Path,
    pattern: str = "shadow_*.tif",
    out_dir: str | Path | None = None,
    mode: str = "continuous",
    dpi: int = 150,
    scale: float = 2.0,
    golden_ratio: bool = True,
    cmap: str | None = None,
    hour_start: int = DAYTIME_HOUR_START,
    hour_end: int = DAYTIME_HOUR_END,
    variable: str | None = None,
    unit: str | None = "°C",
) -> Path | None:
    """Export each SOLWEIG GeoTIFF (or legacy preview PNG) as a separate PNG file."""
    folder = Path(folder)
    variable = variable or folder.name
    out_dir = Path(out_dir) if out_dir else folder / "preview"
    paths = find_raster_paths(folder, pattern)
    if not paths:
        print(f"⚠️  No files found for PNG export: {folder / pattern}")
        return None

    value_range = compute_value_range(paths, hour_start=hour_start, hour_end=hour_end) if cmap else None
    legend_caption = None
    if value_range is not None:
        vmin, vmax = value_range
        legend_caption = format_scale_caption(variable, vmin, vmax, hour_start, hour_end)
        print(f"  {legend_caption}")

    out_dir.mkdir(parents=True, exist_ok=True)
    for path in paths:
        if path.suffix.lower() in {".png", ".jpg", ".jpeg"}:
            img = Image.open(path).convert("RGB")
        else:
            frame_kwargs: dict = {"mode": mode, "cmap": cmap}
            if value_range is not None:
                frame_kwargs["vmin"], frame_kwargs["vmax"] = value_range
            img = _rgba_to_rgb(raster_to_frame(path, **frame_kwargs))

        img = _apply_preview_legend(
            img,
            mode=mode,
            cmap=cmap,
            value_range=value_range,
            legend_caption=legend_caption,
            variable=variable,
            unit=unit,
        )
        img = prepare_png_frame(img, scale=scale, golden_ratio=golden_ratio)
        img.save(out_dir / f"{path.stem}.png", dpi=(dpi, dpi))

    return out_dir


def create_solweig_preview_gifs(
    output_path: Path,
    duration_ms: int = 500,
    hour_start: int = DAYTIME_HOUR_START,
    hour_end: int = DAYTIME_HOUR_END,
) -> None:
    """Build shadow, Tmrt and UTCI preview GIFs from SOLWEIG per-timestep outputs."""
    specs = (
        ("shadow", "shadow_*.tif", "shadow", None, "shadow_preview.gif", None),
        ("tmrt", "tmrt_*.tif", "continuous", "inferno", "tmrt_preview.gif", "°C"),
        ("utci", "utci_*.tif", "continuous", "inferno", "utci_preview.gif", "°C"),
    )
    for subdir, pattern, mode, cmap, gif_name, unit in specs:
        folder = output_path / subdir
        if not folder.is_dir():
            continue
        gif_path = preview_rasters_to_gif(
            folder,
            pattern=pattern,
            out_path=output_path / gif_name,
            duration_ms=duration_ms,
            mode=mode,
            cmap=cmap,
            hour_start=hour_start,
            hour_end=hour_end,
            variable=subdir,
            unit=unit,
        )
        if gif_path is not None:
            print(f"✅ GIF created: {gif_path}")


def create_solweig_preview_pngs(
    output_path: Path,
    dpi: int = 150,
    scale: float = 2.0,
    golden_ratio: bool = True,
    hour_start: int = DAYTIME_HOUR_START,
    hour_end: int = DAYTIME_HOUR_END,
) -> None:
    """Export shadow, Tmrt and UTCI per-timestep rasters as individual PNG previews."""
    specs = (
        ("shadow", "shadow_*.tif", "shadow", None, None),
        ("tmrt", "tmrt_*.tif", "continuous", "inferno", "°C"),
        ("utci", "utci_*.tif", "continuous", "inferno", "°C"),
    )
    for subdir, pattern, mode, cmap, unit in specs:
        folder = output_path / subdir
        if not folder.is_dir():
            continue
        png_dir = export_rasters_to_pngs(
            folder,
            pattern=pattern,
            out_dir=folder / "preview",
            mode=mode,
            dpi=dpi,
            scale=scale,
            golden_ratio=golden_ratio,
            cmap=cmap,
            hour_start=hour_start,
            hour_end=hour_end,
            variable=subdir,
            unit=unit,
        )
        if png_dir is not None:
            n_png = len(list(png_dir.glob("*.png")))
            print(f"✅ {n_png} PNG exportés: {png_dir}")
