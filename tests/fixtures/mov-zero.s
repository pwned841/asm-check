section .text
zero:
    mov rax, 0              ;~
    mov edi, 0              ;~
    mov al, 0
    cmp rbx, 1
    mov rcx, 0
    je .x
    cmp rbx, 1
    mov rdx, 0
    jmp .y
.y:
    jne .x
    mov rsi, 0              ;~
    syscall
.x:
    mov r8, 0               ;~
    ret
