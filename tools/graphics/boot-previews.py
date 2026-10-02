#!/usr/bin/env python3
"""Render exact 480x360 boot/shutdown previews and a contact sheet.

Boot screens come from the real early-splash renderer (tests/splash_picture.c
includes tools/graphics/reborn-splash.c), settled at real milestones. The fade
into Reborn, the shutdown screens and the final black frame come from Reborn's
own UI quads, rasterized by Reborn's preview rasterizer. No device is touched.

Usage: boot-previews.py QUADS_DIR OUTPUT_DIR [--reborn REBORN_ROOT]
  QUADS_DIR  output of `reborn-preview QUADS_DIR` (Y2Reborn)
"""
import argparse
import importlib.util
import json
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
SIZE = (480, 360)

# (file, caption, source, argument)
SHEET = [
    ('01-boot-early', 'boot: early, bar empty', 'splash', ['frame', 'start']),
    ('02-boot-25', 'boot: ~25 % (setting the clock)', 'splash', ['frame', 'rc_time']),
    ('03-boot-60', 'boot: ~60 % (radios up)', 'splash', ['frame', 'conn_wifi']),
    ('04-boot-final-phase', 'boot: final startup phase', 'splash', ['frame', 'runtime_ready']),
    ('05-boot-handoff', 'hand-off: first Reborn frame', 'splash', ['frame', 'ready']),
    ('06a-reveal-bar-out', 'reveal: bar and status out', 'quads', '03a-boot-fade-bar-out'),
    ('06b-reveal-dissolve', 'reveal: wordmark lifts, UI in', 'quads', '03-boot-fade'),
    ('06c-reveal-almost', 'reveal: nearly done', 'quads', '03c-boot-fade-lift'),
    ('07-shutdown-saving', 'shutdown: saving', 'quads', '90-shutdown-b-saving'),
    ('08-shutdown-closing', 'shutdown: shutting down', 'quads', '90-shutdown-c-closing'),
    ('09-final-black', 'final black frame (then backlight off)', 'quads', '94-shutdown-final-black'),
    ('10-boot-failed', 'boot: could not start', 'splash', ['failure']),
]


def reborn_rasterizer(reborn):
    spec = importlib.util.spec_from_file_location('render_previews', reborn / 'tools/preview/render_previews.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    ui_font = module.texture(reborn / 'assets/fonts/reborn-ui.rgba', 1024, 1024)
    icons = module.texture(reborn / 'assets/icons/reborn-icons.rgba', 192, 160)
    artwork = Image.open(reborn / 'docs/ui/fixtures/album-art.png').convert('RGBA')

    def render(path):
        canvas = Image.new('RGBA', SIZE, (9, 11, 13, 255))
        for quad in json.loads(path.read_text())['quads']:
            module.draw_quad(canvas, quad, ui_font, ui_font, icons, artwork)
        return canvas.convert('RGB')
    return render


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('quads', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--reborn', type=Path, default=ROOT.parent / 'Y2Reborn')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'libdrm'], text=True).split()
    render = reborn_rasterizer(args.reborn.resolve())
    with tempfile.TemporaryDirectory() as build:
        binary = Path(build) / 'picture'
        subprocess.run(['cc', '-Os', '-Wall', '-Wextra', '-Werror', str(ROOT / 'tests/splash_picture.c'),
                        *flags, '-o', str(binary)], check=True)
        images = []
        for name, caption, source, argument in SHEET:
            if source == 'splash':
                out = subprocess.check_output([str(binary), *argument])
                image = Image.frombytes('RGB', SIZE, out[len(b'P6\n480 360\n255\n'):])
            else:
                image = render(args.quads / f'{argument}.json')
            assert image.size == SIZE
            image.save(args.output / f'{name}.png')
            images.append((name, caption, image))
    columns = 3
    rows = (len(images) + columns - 1) // columns
    sheet = Image.new('RGB', (SIZE[0] * columns, (SIZE[1] + 26) * rows), (15, 18, 21))
    labels = ImageDraw.Draw(sheet)
    for i, (name, caption, image) in enumerate(images):
        x, y = i % columns * SIZE[0], i // columns * (SIZE[1] + 26)
        labels.text((x + 12, y + 7), f'{name}  {caption}', fill=(242, 240, 235))
        sheet.paste(image, (x, y + 26))
    sheet.save(args.output / 'contact-sheet.png')
    print(f'wrote {len(images)} 480x360 previews and contact-sheet.png to {args.output}')


if __name__ == '__main__':
    main()
