#!/usr/bin/env python3
"""Generate reborn-splash-mark.h from Reborn's boot layout.

The early splash must hand off to Reborn without a visible change, so every
image it shows (the wordmark and each status line) is rendered from Reborn's own
boot-screen quads by Reborn's preview rasterizer, not redrawn here. Only the
region that differs from the background is stored, run-length encoded as
RGB888. The bar is drawn by the splash itself so it can fill smoothly; its
geometry and colours, the startup phase table (milestone name, status line,
coarse bar position) and the failure milestones come from the same layout.

Input is `boot-layout.json`, written by Y2Reborn's `reborn-preview`
(docs/ui/previews/v2/boot-layout.json).

Usage: make-splash-mark.py BOOT_LAYOUT.json OUTPUT.h [--reborn REBORN_ROOT]
"""
import argparse
import importlib.util
import json
from pathlib import Path

from PIL import Image


def rasterizer(reborn):
    spec = importlib.util.spec_from_file_location('render_previews', reborn / 'tools/preview/render_previews.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    ui_font = module.texture(reborn / 'assets/fonts/reborn-ui.rgba', 1024, 1024)
    icons = module.texture(reborn / 'assets/icons/reborn-icons.rgba', 192, 160)
    blank = Image.new('RGBA', (1, 1))

    def render(quads, background):
        canvas = Image.new('RGBA', (480, 360), background + (255,))
        for quad in quads:
            module.draw_quad(canvas, quad, ui_font, ui_font, icons, blank)
        return canvas.convert('RGB')
    return render


def encode(image, background):
    """Bounding box and RGB888 runs of everything that differs from the background."""
    pixels = image.load()
    box = None
    for y in range(image.height):
        for x in range(image.width):
            if pixels[x, y] != background:
                box = (min(box[0], x), min(box[1], y), max(box[2], x), max(box[3], y)) if box else (x, y, x, y)
    if box is None:
        raise SystemExit('image is empty')
    left, top, right, bottom = box
    runs, previous, count = [], None, 0
    for y in range(top, bottom + 1):
        for x in range(left, right + 1):
            colour = pixels[x, y]
            if colour == previous and count < 255:
                count += 1
            else:
                if previous is not None:
                    runs.append((count, *previous))
                previous, count = colour, 1
    runs.append((count, *previous))
    return {'x': left, 'y': top, 'w': right - left + 1, 'h': bottom - top + 1,
            'data': [value for run in runs for value in run]}


def c_string(value):
    return json.dumps(value)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('layout', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--reborn', type=Path)
    args = parser.parse_args()
    layout = json.loads(args.layout.read_text())
    if layout.get('schema') != 'org.reborn.boot-layout/v1' or (layout['width'], layout['height']) != (480, 360):
        raise SystemExit('unsupported boot layout')
    reborn = (args.reborn or args.layout.resolve().parents[4]).resolve()
    render = rasterizer(reborn)
    value = layout['background']
    background = (value >> 16 & 255, value >> 8 & 255, value & 255)

    phases = layout['phases']
    if phases[0]['fill_permille'] != 0 or phases[-1]['fill_permille'] != 1000 or phases[-1]['token'] != 'ready':
        raise SystemExit('phase table must run from 0 to 1000 and end with ready')
    if any(a['fill_permille'] >= b['fill_permille'] for a, b in zip(phases, phases[1:])):
        raise SystemExit('phase fills must strictly increase')

    mark = encode(render(layout['mark'], background), background)
    texts = {}
    for entry in layout['labels']:
        texts[(entry['text'], entry['y'])] = encode(render(entry['quads'], background), background)
    status_y = min(y for _, y in texts)
    status = [image for (text, y), image in texts.items() if y == status_y]
    band = (min(i['x'] for i in status), min(i['y'] for i in status),
            max(i['x'] + i['w'] for i in status), max(i['y'] + i['h'] for i in status))
    # The status band must not overlap the bar or the wordmark.
    bar = layout['bar']
    if band[1] <= bar['y'] + bar['h'] or band[1] <= mark['y'] + mark['h']:
        raise SystemExit('status band overlaps the bar or wordmark')

    images, blob = [], []

    def add(image):
        images.append((image, len(blob)))
        blob.extend(image['data'])
        return len(images) - 1
    mark_index = add(mark)
    label_index = {}
    for key, image in texts.items():
        label_index[key] = add(image)
    status_labels = {text: label_index[(text, y)] for (text, y) in texts if y == status_y}
    failure = [label_index[(line['text'], line['y'])] for line in layout['failure_lines']]

    lines = [', '.join(f'0x{v:02x}' for v in blob[i:i + 16]) for i in range(0, len(blob), 16)]
    image_rows = ',\n'.join(
        f'    {{{image["x"]}u, {image["y"]}u, {image["w"]}u, {image["h"]}u, {offset}u, {len(image["data"])}u}}'
        for image, offset in images)
    phase_rows = ',\n'.join(
        f'    {{{c_string(p["token"])}, {p["fill_permille"]}u, {status_labels[p["label"]]}u}}' for p in phases)
    failure_tokens = ', '.join(c_string(t) for t in layout['failure_tokens'])
    rgb = lambda v: f'0x{v:06x}u'
    args.output.write_text(
        "/* Generated by tools/graphics/make-splash-mark.py from Reborn's boot layout.\n"
        ' * Images are RGB888 runs (count, red, green, blue) decoded from Reborn\'s own\n'
        ' * rendering; the phase table maps milestone names to coarse bar positions.\n'
        ' * Do not edit by hand. */\n'
        '#ifndef REBORN_SPLASH_MARK_H\n#define REBORN_SPLASH_MARK_H\n'
        f'#define RB_BG {rgb(value)}\n'
        f'#define RB_BAR_X {int(bar["x"])}u\n#define RB_BAR_Y {int(bar["y"])}u\n'
        f'#define RB_BAR_W {int(bar["w"])}u\n#define RB_BAR_H {int(bar["h"])}u\n'
        f'#define RB_BAR_TRACK {rgb(bar["track"])}\n#define RB_BAR_FILL {rgb(bar["fill"])}\n'
        f'#define RB_LABEL_X {band[0]}u\n#define RB_LABEL_Y {band[1]}u\n'
        f'#define RB_LABEL_W {band[2] - band[0]}u\n#define RB_LABEL_H {band[3] - band[1]}u\n'
        'typedef struct { unsigned short x, y, w, h; unsigned offset, length; } rb_image;\n'
        'typedef struct { const char *token; unsigned short fill; unsigned char label; } rb_phase;\n'
        'static const unsigned char rb_art_rle[] = {\n    ' + ',\n    '.join(lines) + '\n};\n'
        'static const rb_image rb_images[] = {\n' + image_rows + '\n};\n'
        f'#define rb_mark rb_images[{mark_index}]\n'
        '#define rb_labels rb_images\n'
        'static const rb_phase rb_phases[] = {\n' + phase_rows + '\n};\n'
        f'#define RB_PHASE_COUNT {len(phases)}u\n'
        f'static const char *const rb_failure_tokens[] = {{{failure_tokens}}};\n'
        f'#define RB_FAILURE_TOKEN_COUNT {len(layout["failure_tokens"])}u\n'
        f'static const unsigned char rb_failure_labels[] = {{{", ".join(str(i) for i in failure)}}};\n'
        f'#define RB_FAILURE_LABEL_COUNT {len(failure)}u\n'
        '#endif\n')
    print(f'{args.output}: {len(images)} images, {len(phases)} phases, {len(blob)} bytes of art')


if __name__ == '__main__':
    main()
