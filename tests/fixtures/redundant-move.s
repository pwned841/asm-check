section .text
f:
    mov rax, rbx
    mov rbx, rax            ;~
    mov [value], rax
    mov rax, [value]        ;~
    mov rax, rbx
    mov rcx, rax
    ret

section .data
value dq 0
