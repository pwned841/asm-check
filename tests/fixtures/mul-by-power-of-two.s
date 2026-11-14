section .text
f:
    imul rax, 8             ;~
    imul rcx, rcx, 16       ;~
    imul rdx, 10
    imul rbx, 3
    imul rsi, 2             ;~
    imul rdi, 4
    jo .overflow
    imul r8, 1
    imul eax, 32            ;~
    imul r9, rdi, 8
.overflow:
    ret
