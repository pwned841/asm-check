section .text
jumps:
    jmp .next               ;~
.next:
    jmp .far
    nop
.far:
    ret
