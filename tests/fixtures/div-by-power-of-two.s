section .text
f:
    xor edx, edx
    mov rbx, 8
    mov rax, rdi
    div rbx                 ;~
    xor edx, edx
    mov rax, rdi
    mov rbx, 10
    div rbx
    xor edx, edx
    mov rcx, 4096
    div rcx                 ;~
    mov rbx, 1
    xor edx, edx
    div rbx
    mov rbx, 16
    mov rax, rsi
    div rbx
    ret
