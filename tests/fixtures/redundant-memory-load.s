section .text
f:
    mov rax, [value]
    mov rax, [value]        ;~
    mov rbx, [value]        ;~
    mov rcx, [other]
    mov [value], rdx
    mov rsi, [value]
    mov rdi, [value]        ;~
    call g
    mov r8, [value]
    mov rax, [rax]
    mov rax, [rax]
    mov eax, [value]
    mov eax, [value]        ;~
    ret
g:
    ret

section .data
value dq 0
other dq 0
