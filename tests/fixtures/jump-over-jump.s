section .text
f:
    cmp rax, 1
    jne .skip               ;~
    jmp .target
.skip:
    nop
    je .other
    nop
    jmp .target
.other:
    nop
.target:
    ret
