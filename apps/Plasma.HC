// Plasma.HC — classic demoscene plasma, straight to the framebuffer.
//
// Uses only integer math: a fixed-point sine approximation
//   sin(0..255 degrees) ~= 4*a*(256-a)/256   in the 0..256 range.
//
// Polls Key each frame; any key press puts it back to the shell.

I64 Isin(I64 deg) {
    I64 a;
    a = deg & 255;
    return (4 * a * (256 - a)) >> 8;
}

I64 Main() {
    I64 t;
    I64 x;
    I64 y;
    I64 v;
    I64 w;
    I64 h;
    I64 step;
    I64 key;
    I64 d;

    w = FBW;
    h = FBH;
    Print("plasma %d x %d\n", w, h);
    step = 4;
    t = 0;
    while (1) {
        for (y = 0; y < h; y += step) {
            for (x = 0; x < w; x += step) {
                v = Isin((x * 256) / w + t * 5)
                  + Isin((y * 256) / h + t * 3)
                  + Isin(((x + y) * 256) / (w / 2 + h / 2) - t * 4);
                v = (v + 768) & 15; // fold to a 16 color palette index
                Fill(x, y, step, step, v);
            }
        }
        t++;
        key = Key;
        if (key != -1) {
            break;
        }
    }
    Print("plasma: %d frames then bye\n", t);
    return 0;
}
