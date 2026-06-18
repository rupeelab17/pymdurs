#!/usr/bin/env python3
"""Patch AlexanderWillner/gdal fork for MSVC bindgen (i32 vs u32 enums).

Sur Windows MSVC, bindgen genere les constantes d'enum GDAL en i32, alors que
le code du fork les traite en u32 (repr, discriminants, match arms). Ce script
applique des corrections generales plutot que cas par cas.

Usage: patch_gdal_msvc.py <gdal_fork_src_dir>
"""

from __future__ import annotations

import os
import re
import sys


# Constantes GDAL/OGR generees par bindgen — utilisees comme discriminants
# d'enum ou dans des match arms. Sur MSVC elles sont i32, le fork attend u32.
CONST_PREFIXES = (
    "GDT_", "GDAL", "OAMS_", "OGR", "OAO_", "GFT_", "GFU_", "GRIORA_",
    "GCI_", "GMF_", "GARIO_", "OFTL_", "OFT", "OJUndefined", "wkb",
)


def read(path: str) -> str | None:
    if not os.path.exists(path):
        print(f"SKIP (introuvable): {path}")
        return None
    with open(path, encoding="utf-8") as f:
        return f.read()


def write(path: str, content: str) -> None:
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)
    print(f"Patched: {path}")


def patch_enum_discriminants(content: str) -> str:
    """Ajoute 'as u32' aux discriminants d'enum de la forme:
        Variant = SOMECONST::CONST_NAME,
    et aussi:
        Variant = bare_const_name,
    """
    # Forme : = NAMESPACE::CONST,  ou  = NAMESPACE::CONST }
    def repl_path(m: re.Match) -> str:
        return f"{m.group(1)} as u32{m.group(2)}"

    # = Xxx::YYY suivi de , ou } ou fin de ligne, sans deja "as u32"
    content = re.sub(
        r"(=\s*[A-Za-z_][A-Za-z0-9_]*::[A-Z][A-Za-z0-9_]*)(\s*[,}])",
        repl_path,
        content,
    )
    return content


def patch_match_arms(content: str) -> str:
    """Transforme les match arms qui comparent une valeur u32 a des constantes
    i32 en guards. Gere les arms simples et les arms multiples (A | B | C =>).

    Avant:  GDT_Byte => ...
            GDT_CInt16 | GDT_CInt32 => ...
    Apres:  x if x == GDT_Byte as u32 => ...
            x if x == GDT_CInt16 as u32 || x == GDT_CInt32 as u32 => ...
    """
    const_re = "|".join(CONST_PREFIXES)
    # Une ligne d'arm : indentation + (CONST | CONST | ...) => reste
    arm_re = re.compile(
        r"^(?P<indent>\s+)"
        r"(?P<consts>(?:[A-Za-z_][A-Za-z0-9_]*)(?:\s*\|\s*[A-Za-z_][A-Za-z0-9_]*)*)"
        r"(?P<arrow>\s*=>)",
        re.MULTILINE,
    )

    def is_const(tok: str) -> bool:
        return tok.startswith(CONST_PREFIXES)

    def repl(m: re.Match) -> str:
        consts_raw = m.group("consts")
        tokens = [t.strip() for t in consts_raw.split("|")]
        # Ne patche que si TOUS les tokens sont des constantes GDAL connues
        if not tokens or not all(is_const(t) for t in tokens):
            return m.group(0)
        guards = " || ".join(f"x == {t} as u32" for t in tokens)
        return f"{m.group('indent')}x if {guards}{m.group('arrow')}"

    return arm_re.sub(repl, content)


def patch_file_general(path: str, guard: str) -> None:
    content = read(path)
    if content is None:
        return
    if guard and guard in content:
        print(f"Already patched (general): {path}")
        return
    content = patch_enum_discriminants(content)
    content = patch_match_arms(content)
    # Marqueur d'idempotence en tete de fichier
    content = f"// {guard}\n" + content
    write(path, content)


def append_impls(path: str, guard: str, lines: list[str]) -> None:
    content = read(path)
    if content is None:
        return
    if guard in content:
        print(f"Impl already present: {path}")
        return
    content += "\n" + "\n".join(lines) + "\n"
    write(path, content)
    print(f"Impl appended: {path}")


def main() -> int:
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <gdal_fork_src_dir>", file=sys.stderr)
        return 1

    src = sys.argv[1]
    srs = os.path.join(src, "src", "spatial_ref", "srs.rs")
    types = os.path.join(src, "src", "raster", "types.rs")
    rband = os.path.join(src, "src", "raster", "rasterband.rs")

    # 1. err_code : u32 -> OGRErr::Type (cross-platform)
    content = read(srs)
    if content is not None and "gdal_sys::OGRErr::Type" not in content:
        content = re.sub(
            r"let mut err_code:\s*u32\b",
            "let mut err_code: gdal_sys::OGRErr::Type",
            content,
        )
        content = re.sub(r"&mut err_code as \*mut u32\b", "&mut err_code", content)
        write(srs, content)

    # 2. Patch general discriminants + match arms sur les 3 fichiers
    for path in (srs, types, rband):
        patch_file_general(path, guard="__CI_MSVC_PATCHED__")

    # 2b. Corrections inverses : certains "as u32" doivent rester i32 sur MSVC
    # car ils sont passes a des fonctions C (GDALCreate) ou compares a des i32
    # (gdal_ordinal). Sur MSVC bindgen, ces types C sont i32.
    driver = os.path.join(src, "src", "driver.rs")
    for path in (driver, rband, types, srs):
        c = read(path)
        if c is None:
            continue
        original = c
        # data_type as u32 -> data_type as gdal_sys::GDALDataType::Type (i32 MSVC)
        c = re.sub(
            r"\bdata_type as u32\b",
            "data_type as gdal_sys::GDALDataType::Type",
            c,
        )
        # self.band_type() as u32 -> self.band_type() as i32 (compare a gdal_ordinal i32)
        c = re.sub(
            r"self\.band_type\(\) as u32\b",
            "self.band_type() as gdal_sys::GDALDataType::Type",
            c,
        )
        if c != original:
            write(path, c)
            print(f"Corrections inverses appliquees: {path}")

    # 3. TryFrom<i32> pour les enums utilises avec try_into() sur valeur i32
    append_impls(
        srs,
        "TryFrom<i32> for AxisMappingStrategy",
        [
            "// __CI_MSVC_PATCHED__ impl ajoutee par le CI",
            '#[cfg(target_env = "msvc")]',
            "impl TryFrom<i32> for AxisMappingStrategy {",
            "    type Error = <AxisMappingStrategy as TryFrom<u32>>::Error;",
            "    fn try_from(v: i32) -> core::result::Result<Self, Self::Error> { Self::try_from(v as u32) }",
            "}",
        ],
    )
    append_impls(
        types,
        "TryFrom<i32> for GdalDataType",
        [
            "// __CI_MSVC_PATCHED__ impl ajoutee par le CI",
            '#[cfg(target_env = "msvc")]',
            "impl TryFrom<i32> for GdalDataType {",
            "    type Error = <GdalDataType as TryFrom<u32>>::Error;",
            "    fn try_from(v: i32) -> core::result::Result<Self, Self::Error> { Self::try_from(v as u32) }",
            "}",
        ],
    )

    print("All patches done")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())