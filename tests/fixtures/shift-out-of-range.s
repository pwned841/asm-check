section .text
f:
    shl rax, 64                 ;~
    shl rax, 63
    shr eax, 32                 ;~
    sar al, 8                   ;~
    shl al, 7
    rol rax, 65                 ;~
    shl ax, 16                  ;~
    shl rax, cl
    ret
