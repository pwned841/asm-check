section .text
f:
    mov ecx, 4
.again:
    dec rax
    loop .again             ;~
    ret
