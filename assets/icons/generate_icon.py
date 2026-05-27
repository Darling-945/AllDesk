"""Generate AllDesk app icon - modern remote desktop icon."""
from PIL import Image, ImageDraw, ImageFont
import math

def rrect(draw, xy, radius, **kwargs):
    """Draw a rounded rectangle, clamping radius."""
    x0, y0, x1, y1 = xy
    max_r = min(x1 - x0, y1 - y0) / 2
    radius = min(radius, max_r)
    if radius < 1:
        draw.rectangle(xy, **kwargs)
    else:
        draw.rounded_rectangle(xy, radius=int(radius), **kwargs)

def create_icon(size):
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    s = size

    # === Background: gradient rounded square ===
    bg = Image.new('RGBA', (s, s), (0, 0, 0, 0))
    bg_draw = ImageDraw.Draw(bg)
    for y in range(s):
        t = y / s
        r = int(18 + t * 12)
        g = int(75 + t * 95)
        b = int(195 - t * 45)
        bg_draw.line([(0, y), (s, y)], fill=(r, g, b, 255))

    mask = Image.new('L', (s, s), 0)
    mask_draw = ImageDraw.Draw(mask)
    r = int(s * 0.22)
    mask_draw.rounded_rectangle([0, 0, s - 1, s - 1], radius=r, fill=255)
    bg.putalpha(mask)
    img = bg
    draw = ImageDraw.Draw(img)

    # === Monitor ===
    mx, my = s * 0.20, s * 0.16
    mw, mh = s * 0.60, s * 0.48
    mr = max(1, int(s * 0.04))

    # Monitor bezel
    rrect(draw, [mx - 2, my - 2, mx + mw + 2, my + mh + 2], mr + 2, fill=(40, 50, 75, 255))
    # Screen
    rrect(draw, [mx, my, mx + mw, my + mh], mr, fill=(18, 22, 38, 255))

    # Screen inner content - gradient
    sp = max(1, int(s * 0.025))
    sx1, sy1 = mx + sp, my + sp
    sx2, sy2 = mx + mw - sp, my + mh - sp
    for y in range(int(sy1), int(sy2)):
        t = (y - sy1) / max(1, sy2 - sy1)
        cr = int(35 + t * 25)
        cg = int(55 + t * 55)
        cb = int(110 + t * 40)
        draw.line([(sx1, y), (sx2, y)], fill=(cr, cg, cb, 220))

    # Taskbar
    tb_h = max(1, (sy2 - sy1) * 0.14)
    tb_y = sy2 - tb_h
    draw.rectangle([sx1, tb_y, sx2, sy2], fill=(12, 16, 30, 230))

    # Start button dot
    dot_size = max(1, int(tb_h * 0.45))
    dot_x = sx1 + (sx2 - sx1) * 0.06
    dot_y = tb_y + tb_h * 0.5
    draw.ellipse([dot_x - dot_size/2, dot_y - dot_size/2,
                  dot_x + dot_size/2, dot_y + dot_size/2],
                 fill=(0, 200, 160, 255))

    # Small windows
    ww = (sx2 - sx1) * 0.38
    wh = (sy2 - sy1) * 0.40
    wr = max(1, int(s * 0.01))
    rrect(draw, [sx1 + (sx2-sx1)*0.06, sy1 + (sy2-sy1)*0.06,
                  sx1 + (sx2-sx1)*0.06 + ww, sy1 + (sy2-sy1)*0.06 + wh],
           wr, fill=(42, 52, 78, 170))
    rrect(draw, [sx1 + (sx2-sx1)*0.28, sy1 + (sy2-sy1)*0.14,
                  sx1 + (sx2-sx1)*0.28 + ww, sy1 + (sy2-sy1)*0.14 + wh],
           wr, fill=(52, 62, 90, 170))

    # === Stand ===
    st_w = max(2, s * 0.06)
    st_x = s / 2 - st_w / 2
    st_top = my + mh
    st_bot = s * 0.75
    draw.rectangle([st_x, st_top, st_x + st_w, st_bot], fill=(45, 55, 80, 255))

    # Base
    ba_w = max(4, s * 0.20)
    ba_x = s / 2 - ba_w / 2
    ba_y = st_bot
    ba_h = max(1, s * 0.03)
    rrect(draw, [ba_x, ba_y, ba_x + ba_w, ba_y + ba_h], max(1, ba_h/2), fill=(45, 55, 80, 255))

    # === Connection signal (bottom-right) ===
    cx, cy = s * 0.80, s * 0.85
    for i in range(3):
        r_arc = s * (0.04 + i * 0.035)
        alpha = 220 - i * 50
        lw = max(1, int(s * 0.02))
        draw.arc([cx - r_arc, cy - r_arc, cx + r_arc, cy + r_arc],
                 start=210, end=330, fill=(0, 230, 190, alpha), width=lw)

    # Center dot
    dr = max(1, s * 0.025)
    draw.ellipse([cx - dr, cy - dr, cx + dr, cy + dr], fill=(0, 255, 200, 255))

    # === "AD" text ===
    if s >= 64:
        try:
            fs = max(8, int(s * 0.14))
            font = ImageFont.truetype("arial.ttf", fs)
        except:
            font = ImageFont.load_default()
        text = "AD"
        bbox_t = draw.textbbox((0, 0), text, font=font)
        tw = bbox_t[2] - bbox_t[0]
        tx = s * 0.10
        ty = s * 0.10
        draw.text((tx + 1, ty + 1), text, fill=(0, 0, 0, 80), font=font)
        draw.text((tx, ty), text, fill=(255, 255, 255, 220), font=font)

    return img


if __name__ == '__main__':
    import os
    out_dir = os.path.dirname(os.path.abspath(__file__))

    sizes = [16, 32, 48, 64, 128, 256, 512, 1024]
    for sz in sizes:
        icon = create_icon(sz)
        icon.save(os.path.join(out_dir, f'icon_{sz}.png'))
        print(f'icon_{sz}.png')

    # Main icon
    create_icon(512).save(os.path.join(out_dir, 'icon.png'))
    print('icon.png (512x512) - Done!')
