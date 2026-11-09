// Hello.HC — first HolyC app for the Rust-TempleOS AOT compiler.
//
// Compile: hcc Hello.HC hello.bin --api build/kernel_api.nm
// Run:     Run Hello;   (or just: Hello;)
//
// Exercises: calls, globals, locals, pointers, indexing, formats.

U64 Visits; // zero-initialized global, lives in the blob data section

U0 Greet(U8 *who) {
    Print("Hello, %s!\n", who);
}

I64 Main() {
    U8 buf[24];
    U8 *p;
    U64 i;

    Greet("HolyC-AOT");
    Print("fb is %d x %d at %x\n", FBW, FBH, FBADDR);

    Visits += 1; // compound assignment on a global
    Print("Visits: %d\n", Visits);

    // pointer + index stores, byte at a time
    p = buf;
    for (i = 0; i < 12; i++) {
        p[i] = 'A' + i;
    }
    p[12] = 0; // NUL-terminate
    Print("buf: %s\n", buf);

    Print("chars %c%c digits %d hex %x %X\n", 'X', 'Y', 42, 0xABCDEF, 255);
    "bare string statements call Print for you\n";

    Snd(440);
    Sleep(200);
    Print("bye!\n");
    return 0;
}
