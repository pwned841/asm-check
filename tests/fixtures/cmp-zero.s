section .text
compare:
    cmp rax, 0              ;~
    cmp al, 0               ;~
    cmp byte [rdi], 0
    cmp rax, 1
    test rax, rax
    ret
