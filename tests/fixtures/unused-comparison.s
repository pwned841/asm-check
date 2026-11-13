section .text
check:
    cmp rax, 5              ;~
    cmp rax, 6
    je .a
    test rbx, rbx           ;~
    add rbx, 1
    mov rcx, 2
    ret
.a:
    cmp rax, 1              ;~
    xor eax, eax
    ret
.b:
    cmp rax, 2
    ret
