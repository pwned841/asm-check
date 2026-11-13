section .text
noops:
    add rax, 0              ;~
    sub rbx, 0              ;~
    shl rcx, 0              ;~
    xor rax, 0              ;~
    and rdx, -1             ;~
    and rax, 0xFFFFFFFFFFFFFFFF ;~
    imul rax, 1             ;~
    add rax, 1
    and rax, 1
    ret
