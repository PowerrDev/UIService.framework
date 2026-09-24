#!/usr/bin/env python3
"""Create a minimal UIService app and register it in the Cargo workspace."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def rust_type(package: str) -> str:
    words = re.split(r"[^A-Za-z0-9]+", package)
    name = "".join(word[:1].upper() + word[1:] for word in words if word)
    if not name:
        raise SystemExit("package must contain letters or digits")
    if name[0].isdigit():
        name = "App" + name
    return name + "App"


def register_workspace(package: str) -> None:
    manifest = ROOT / "Cargo.toml"
    text = manifest.read_text()
    member = f'    "apps/{package}",\n'
    dependency = f'{package} = {{ path = "apps/{package}" }}\n'

    if member not in text:
        marker = '    "crates/ui",\n'
        text = text.replace(marker, member + marker, 1)

    if dependency not in text:
        marker = 'ui = { path = "crates/ui" }\n'
        text = text.replace(marker, dependency + marker, 1)

    manifest.write_text(text)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", required=True)
    parser.add_argument("--name", required=True)
    parser.add_argument("--identifier", required=True)
    parser.add_argument("--width", type=int, default=640)
    parser.add_argument("--height", type=int, default=420)
    args = parser.parse_args()

    if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", args.package):
        raise SystemExit("package must use lowercase letters, digits and hyphens")
    if args.width <= 0 or args.height <= 0:
        raise SystemExit("window dimensions must be positive")

    app_dir = ROOT / "apps" / args.package
    if app_dir.exists():
        raise SystemExit(f"{app_dir.relative_to(ROOT)} already exists")

    app_dir.joinpath("src").mkdir(parents=True)
    app_dir.joinpath("Cargo.toml").write_text(
        f'''[package]\nname = "{args.package}"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\n\n[dependencies]\nui.workspace = true\n'''
    )

    type_name = rust_type(args.package)
    app_dir.joinpath("src/lib.rs").write_text(
        f'''#![no_std]\n\nuse ui::prelude::*;\n\npub struct {type_name};\n\nimpl App for {type_name} {{\n    const INFO: AppInfo<'static> =\n        AppInfo::new("{args.name}", "{args.identifier}", "0.1.0");\n    const WINDOW: WindowConfig = WindowConfig::new({args.width}, {args.height});\n\n    fn draw(&mut self, ui: &mut Frame<'_>) {{\n        ui.fill(system_color::WINDOW_BACKGROUND);\n        ui.text_semibold(\n            Point::new(scale::pt_i32(24), scale::pt_i32(28)),\n            "{args.name}",\n            system_color::LABEL,\n            scale::pt(24),\n        );\n    }}\n}}\n'''
    )
    register_workspace(args.package)
    print(f"Created apps/{args.package}")
    print(f"App ID: {args.identifier}")


if __name__ == "__main__":
    main()
