/* Stage 1: 512-byte MBR. Loads stage 2 with INT 13h LBA reads and jumps. */
.code16
.section .text
.global _start
_start:
    cli
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw $0x7c00, %sp

    movb %dl, boot_drive

    movw $dib, %si
    movb boot_drive, %dl
    movb $0x42, %ah
    int $0x13
    jc .Lfail

    movb boot_drive, %dl
    ljmp $0x0000, $0x8000

.Lfail:
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

msg:
    .asciz "Disk read error\r\n"

boot_drive:
    .byte 0

    .align 4
dib:
    .byte 16, 0
    .word STAGE2_SECTORS
    .word 0x8000
    .word 0x0000
    .quad 1

    .org 510
    .word 0xaa55
