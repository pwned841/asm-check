section .text
signed:
    xor edx, edx
    idiv rcx                ;~
    cqo
    idiv rcx
    mov rdx, 0
    idiv rcx                ;~
    xor edx, edx
    div rcx
    ret
