section .text
f:
    cmp rax, rax            ;~
    test rax, 0             ;~
    test byte [rdi], 0      ;~
    cmp rax, rbx
    test rax, rax
    test rax, 1
    ret
