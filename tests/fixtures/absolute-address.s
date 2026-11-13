section .text
f:
    mov rax, [value]        ;~
    mov rax, [rel value]
    lea rsi, [value]        ;~
    mov rax, [rbx]
    mov rax, [value + 8]
    ret

section .data
value dq 0, 0
