/*
 * Stage 2.
 *
 *   1. enable A20
 *   2. set the largest VBE linear-framebuffer mode up to 1920x1080
 *      (falls back to VGA mode 12h, 640x480x16)
 *   3. stash a BootInfo struct at 0x5000
 *   4. load the kernel at 0x10000 with INT 13h LBA reads
 *   5. build identity page tables at 0x90000 (4 GiB, 2 MiB pages)
 *   6. enter 64-bit long mode and jump to 0x10000
 *
 * Build-time constants (passed with -D):
 *   KERNEL_LBA, KERNEL_SECTORS
 */
.code16
.section .text
.global _stage2
_stage2:
    cli
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw $0x7c00, %sp
    movb %dl, boot_drive

    /* --- A20 gate, fast method --- */
    inb $0x92, %al
    orb $0x02, %al
    andb $0xfe, %al
    outb %al, $0x92

    /* --- VBE: pick the largest linear-framebuffer mode up to 1920x1080 --- */
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw $0x6000, %di
    movw $0x4F00, %ax
    int $0x10
    pushw %ax
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    popw %ax
    cmpw $0x004F, %ax
    jne .Lvbe_fail
    cmpl $0x41534556, 0x6000          /* "VESA" */
    jne .Lvbe_fail

    /* copy the mode list to 0x6400 so DS can stay zero while we iterate */
    movw 0x600E, %si
    movw 0x6010, %ax
    movw %ax, %ds
    xorw %ax, %ax
    movw %ax, %es
    movw $0x6400, %di
    movw $256, %cx
.Lcopy_modes:
    movw (%si), %ax
    movw %ax, %es:(%di)
    addw $2, %si
    addw $2, %di
    cmpw $0xFFFF, %ax
    je .Lcopied
    loop .Lcopy_modes
.Lcopied:
    xorw %ax, %ax
    movw %ax, %ds

    movl $0, best_score
    movw $0xFFFF, best_mode
    movw $0x6400, %si
.Lmode_loop:
    movw (%si), %cx
    cmpw $0xFFFF, %cx
    je .Lmode_done
    addw $2, %si
    movw %cx, cur_mode

    pushw %si
    xorw %ax, %ax
    movw %ax, %es
    movw $0x6200, %di
    movw $0x4F01, %ax
    int $0x10
    pushw %ax
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    popw %ax
    popw %si
    cmpw $0x004F, %ax
    jne .Lmode_loop

    /* supported + graphics + linear framebuffer, single plane, 16/24/32 bpp */
    movw 0x6200, %ax
    andw $0x0091, %ax
    cmpw $0x0091, %ax
    jne .Lmode_loop
    cmpb $1, 0x6218
    jne .Lmode_loop
    movzbl 0x6219, %eax
    cmpl $16, %eax
    jb .Lmode_loop
    cmpl $32, %eax
    ja .Lmode_loop

    movzwl 0x6212, %ebx             /* x resolution */
    cmpl $1920, %ebx
    ja .Lmode_loop
    testl %ebx, %ebx
    jz .Lmode_loop
    movzwl 0x6214, %ecx             /* y resolution */
    cmpl $1080, %ecx
    ja .Lmode_loop
    testl %ecx, %ecx
    jz .Lmode_loop

    /* score = area * 4 + bpp, so larger wins and 32 bpp breaks ties */
    movl %ebx, %edx
    imull %ecx, %edx
    shll $2, %edx
    addl %eax, %edx
    cmpl best_score, %edx
    jbe .Lmode_loop
    movl %edx, best_score
    movw cur_mode, %ax
    movw %ax, best_mode
    jmp .Lmode_loop

.Lmode_done:
    cmpw $0xFFFF, best_mode
    je .Lvbe_fail

    movw best_mode, %bx
    orw $0x4000, %bx                /* request linear framebuffer */
    movw $0x4F02, %ax
    int $0x10
    pushw %ax
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    popw %ax
    cmpw $0x004F, %ax
    jne .Lvbe_fail

    /* re-query the chosen mode for its exact fields */
    movw best_mode, %cx
    xorw %ax, %ax
    movw %ax, %es
    movw $0x6200, %di
    movw $0x4F01, %ax
    int $0x10
    pushw %ax
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    popw %ax
    cmpw $0x004F, %ax
    jne .Lvbe_fail

    movl 0x6228, %eax               /* PhysBasePtr */
    movl %eax, 0x5010
    movl $0, 0x5014
    movzwl 0x6212, %eax             /* XResolution */
    movl %eax, 0x5018
    movzwl 0x6214, %eax             /* YResolution */
    movl %eax, 0x501C
    movzwl 0x6232, %eax             /* LinBytesPerScanLine */
    testl %eax, %eax
    jnz 1f
    movzwl 0x6210, %eax             /* BytesPerScanLine */
1:
    movl %eax, 0x5020
    movzbl 0x6219, %eax             /* BitsPerPixel */
    movl %eax, 0x5024
    movw best_mode, %ax
    movw %ax, 0x5028
    movw $1, 0x502A                 /* linear framebuffer */
    jmp .Lvbe_ok

.Lvbe_fail:
    /* no VBE: fall back to VGA mode 12h (640x480x16, planar) */
    movw $0x0012, %ax
    int $0x10
    xorw %ax, %ax
    movw %ax, %ds
    movw $0x0012, %ax
    movw %ax, 0x5004
    movl $0, 0x5010
    movl $0, 0x5014
    movl $640, 0x5018
    movl $480, 0x501C
    movl $80, 0x5020
    movl $4, 0x5024
    movw $0x12, %ax
    movw %ax, 0x5028
    movw $0, 0x502A                 /* planar VGA */

.Lvbe_ok:

    /* --- ask the BIOS where its 8x16 font lives --- */
    movw $0x1130, %ax
    movb $0x06, %bh
    int $0x10
    xorl %eax, %eax
    movw %es, %ax
    shll $4, %eax
    movzwl %bp, %ebx
    addl %ebx, %eax
    xorw %bx, %bx
    movw %bx, %ds
    movl %eax, 0x5000

    /* --- BootInfo at 0x5000 --- */
    movw $0x0012, %ax
    movw %ax, 0x5004
    movzbl boot_drive, %eax
    movw %ax, 0x5006
    movl $KERNEL_SECTORS * 512, %eax
    movl %eax, 0x5008
    movl $0x544f5331, %eax          /* "TOS1" */
    movl %eax, 0x500c

    /* --- load the kernel --- */
    movw $0x1000, %ax               /* 0x10000 */
    movw %ax, dest_seg
    movl $KERNEL_LBA, %eax
    movl %eax, cur_lba
    movl $KERNEL_SECTORS, %eax
    movl %eax, remaining

.Lload:
    movl remaining, %eax
    testl %eax, %eax
    jz .Lloaded
    cmpl $64, %eax
    jbe 1f
    movl $64, %eax
1:
    movw %ax, dib_count
    movw dest_seg, %bx
    movw %bx, dib_seg
    movl cur_lba, %eax
    movl %eax, dib_lba
    movl $0, dib_lba + 4

    xorw %ax, %ax
    movw %ax, %ds
    movw $dib, %si
    movb boot_drive, %dl
    movb $0x42, %ah
    int $0x13
    jc .Ldisk
    xorw %ax, %ax
    movw %ax, %ds

    movzwl dib_count, %eax
    movw %ax, %bx
    shlw $5, %bx                    /* sectors * 32 paragraphs */
    addw %bx, dest_seg
    subl %eax, remaining
    addl %eax, cur_lba
    jmp .Lload

.Ldisk:
    movw $msg, %si
1:
    lodsb
    testb %al, %al
    jz 2f
    movb $0x0e, %ah
    movw $0x0007, %bx
    int $0x10
    jmp 1b
2:
    cli
    hlt
    jmp 2b

/* ---------------- protected mode ---------------- */
.Lloaded:
    cli
    lgdt gdt_desc
    movl %cr0, %eax
    orl $1, %eax
    movl %eax, %cr0
    ljmpl $0x08, $pm32

.code32
pm32:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw %ax, %fs
    movw %ax, %gs
    movl $0x88000, %esp

    /* zero six pages of tables at 0x90000: PML4, PDPT, four PDs */
    movl $0x90000, %edi
    xorl %eax, %eax
    movl $(6 * 1024), %ecx
    rep stosl

    movl $0x91003, 0x90000          /* PML4[0] -> PDPT */
    movl $0, 0x90004

    /* PDPT[0..3] -> four PDs, covering 4 GiB */
    movl $0x92003, 0x91000
    movl $0, 0x91004
    movl $0x93003, 0x91008
    movl $0, 0x9100c
    movl $0x94003, 0x91010
    movl $0, 0x91014
    movl $0x95003, 0x91018
    movl $0, 0x9101c

    /* four PDs: 2048 entries, 2 MiB each, identity map 4 GiB */
    movl $0x92000, %edi
    movl $0x83, %eax
    movl $2048, %ecx
1:
    movl %eax, (%edi)
    movl $0, 4(%edi)
    addl $0x200000, %eax
    addl $8, %edi
    loop 1b

    movl %cr4, %eax
    orl $0x620, %eax                /* PAE | OSFXSR | OSXMMEXCPT */
    movl %eax, %cr4

    movl $0x90000, %eax
    movl %eax, %cr3                 /* PML4 */

    movl $0xc0000080, %ecx
    rdmsr
    orl $0x100, %eax                /* EFER.LME */
    wrmsr

    movl %cr0, %eax
    orl $0x80000000, %eax           /* paging */
    movl %eax, %cr0

    ljmpl $0x18, $pm64

.code64
pm64:
    movw $0x20, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    xorw %ax, %ax
    movw %ax, %fs
    movw %ax, %gs
    movq $0x87ff8, %rsp             /* SysV: rsp % 16 == 8 at entry */
    xorq %rbp, %rbp
    movq $0x10000, %rax
    jmpq *%rax

.align 8
gdt:
    .quad 0x0000000000000000
    .quad 0x00cf9a000000ffff        /* 0x08: 32-bit code */
    .quad 0x00cf92000000ffff        /* 0x10: data */
    .quad 0x00af9a000000ffff        /* 0x18: 64-bit code */
    .quad 0x00cf92000000ffff        /* 0x20: data */
gdt_end:
gdt_desc:
    .word gdt_end - gdt - 1
    .long gdt

msg:
    .asciz "Disk read error\r\n"

boot_drive:
    .byte 0

    .align 4
best_mode:
    .word 0
best_score:
    .long 0
cur_mode:
    .word 0

    .align 4
dib:
    .byte 16, 0
dib_count:
    .word 0
dib_off:
    .word 0
dib_seg:
    .word 0
dib_lba:
    .quad 0

dest_seg:
    .word 0
cur_lba:
    .long 0
remaining:
    .long 0
