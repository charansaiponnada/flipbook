"""Draws the Flipbook icon, the .ico and the installer panel. Run from the repo root: python assets/windows/icon.py"""
from PIL import Image, ImageDraw, ImageFilter

SS = 4  # supersampling: PIL polygons aren't antialiased, so draw big and shrink

def bez(p0, p1, p2, n=40):
    return [((1-t)**2*p0[0] + 2*(1-t)*t*p1[0] + t*t*p2[0],
             (1-t)**2*p0[1] + 2*(1-t)*t*p1[1] + t*t*p2[1]) for t in (i/n for i in range(n+1))]

def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))

def draw(size, detail=True):
    W = 1024 * SS
    s = lambda pts: [(x*SS, y*SS) for x, y in pts]
    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))

    # rounded tile with a warm top-to-bottom gradient
    m, r = 56, 220
    grad = Image.new("RGBA", (W, W))
    gd = ImageDraw.Draw(grad)
    top, bot = (190, 106, 62), (118, 56, 28)
    for y in range(W):
        gd.line([(0, y), (W, y)], fill=lerp(top, bot, y / W) + (255,))
    mask = Image.new("L", (W, W), 0)
    ImageDraw.Draw(mask).rounded_rectangle([m*SS, m*SS, (1024-m)*SS, (1024-m)*SS], r*SS, fill=255)
    img.paste(grad, (0, 0), mask)
    # soft top highlight
    hl = Image.new("L", (W, W), 0)
    hd = ImageDraw.Draw(hl)
    for y in range(W // 2):
        hd.line([(0, y), (W, y)], fill=round(44 * (1 - y / (W / 2)) ** 2))
    hl = Image.composite(hl, Image.new("L", (W, W), 0), mask)
    img = Image.composite(Image.new("RGBA", (W, W), (255, 255, 255, 255)), img, hl)

    spine_t, spine_b = (512, 330), (512, 812)
    left = bez(spine_t, (360, 268), (178, 290)) + bez((178, 762), (360, 742), spine_b)
    right = bez(spine_t, (664, 268), (846, 290)) + bez((846, 762), (664, 742), spine_b)

    # shadow under the book
    sh = Image.new("L", (W, W), 0)
    ImageDraw.Draw(sh).polygon(s([(x, y + 26) for x, y in left + right[::-1]]), fill=120)
    sh = sh.filter(ImageFilter.GaussianBlur(22 * SS))
    img = Image.composite(Image.new("RGBA", (W, W), (60, 24, 8, 255)), img, Image.composite(sh, Image.new("L", (W, W), 0), mask))

    d = ImageDraw.Draw(img)
    d.polygon(s(left), fill=(255, 251, 243))
    d.polygon(s(right), fill=(244, 234, 216))

    if detail:
        # lines of text on the left page
        for i in range(7):
            y = 372 + i * 52
            pts = bez((232, y + 16 - i), (360, y - 8), (470, y + 26), 20)
            d.line(s(pts), fill=(200, 176, 150), width=10 * SS)
        # the page being turned: lifts off the right page toward the reader
        turn = bez(spine_t, (600, 250), (742, 196)) + bez((742, 196), (690, 470), (770, 708)) + bez((770, 708), (640, 728), spine_b)
        tsh = Image.new("L", (W, W), 0)
        ImageDraw.Draw(tsh).polygon(s([(x + 30, y + 18) for x, y in turn]), fill=90)
        tsh = tsh.filter(ImageFilter.GaussianBlur(16 * SS))
        rmask = Image.new("L", (W, W), 0)
        ImageDraw.Draw(rmask).polygon(s(right), fill=255)
        img = Image.composite(Image.new("RGBA", (W, W), (150, 110, 70, 255)), img, Image.composite(tsh, Image.new("L", (W, W), 0), rmask))
        d = ImageDraw.Draw(img)
        d.polygon(s(turn), fill=(255, 253, 248))
        # fold shading near the spine
        d.polygon(s(bez(spine_t, (560, 290), (600, 262)) + bez((600, 262), (585, 540), (575, 780)) + [spine_b]), fill=(238, 226, 206))

    d.line(s([spine_t, spine_b]), fill=(126, 62, 32), width=8 * SS)
    return img.resize((size, size), Image.LANCZOS)

if __name__ == "__main__":
    big = draw(1024)
    big.save("app-icon.png")
    # small sizes skip the fine detail, which only turns to mush below 48 px
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    imgs = [draw(n, detail=n >= 48) for n in sizes]
    imgs[-1].save("assets/windows/flipbook.ico", sizes=[(n, n) for n in sizes], append_images=imgs[:-1])

    # installer welcome/finish panel (MUI2 wants 164x314 BMP)
    side = Image.new("RGB", (164, 314))
    sd = ImageDraw.Draw(side)
    for y in range(314):
        sd.line([(0, y), (164, y)], fill=lerp((190, 106, 62), (96, 44, 22), y / 314))
    ic = draw(112)
    side.paste(ic, (26, 70), ic)
    side.save("assets/windows/installer-side.bmp")
    print("ok")
