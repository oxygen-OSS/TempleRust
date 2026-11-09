ENTRY(_start)

SECTIONS
{
    . = 0x10000;

    .text : ALIGN(16) {
        KEEP(*(.text.entry))
        *(.text .text.*)
    }

    .rodata : ALIGN(16) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(16) {
        *(.data .data.*)
    }

    .bss : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* Fixed-address HolyC API. AOT-compiled apps CALL these by absolute
       address, so they must not move between builds (see hcapi.rs).
       Statics come first, at 0x68000, in their own writable section;
       functions follow. */
    . = 0x68000;
    .hcapi.data : ALIGN(16) {
        KEEP(*(.hcapi.d))
    }
    .hcapi : ALIGN(16) {
        KEEP(*(.hcapi))
        KEEP(*(.hcapi.*))
    }

    /DISCARD/ : {
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.comment)
        *(.note .note.*)
    }
}
