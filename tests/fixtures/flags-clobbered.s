section .text
check:
    cmp rax, 5
    inc rcx
    je .a                   ;~
    cmp rax, 5
    inc rcx
    jb .a
    cmp rax, 5
    add rcx, 1
    jl .a                   ;~
    cmp rax, 5
    mov rcx, 1
    lea rdx, [rcx + 1]
    jne .a
    dec rcx
    jnz .a
    cmp rax, 5
    call helper
    jg .a
.a:
    ret
helper:
    ret
