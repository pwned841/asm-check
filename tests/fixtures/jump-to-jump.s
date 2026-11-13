section .text
f:
    cmp rax, 1
    je .hop                 ;~
    jmp .hop                ;~
.hop:
    jmp .end
.self:
    jmp .self
.end:
    ret
