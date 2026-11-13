global _start
section .text
_start:
    mov rax, 1              ;~
    mov rax, 2
    mov rbx, rax
    mov rcx, 5              ;~
    mov rdi, 1
    mov rax, 1
    syscall
    xor edx, edx            ;~
    mov rdx, 4
    mov rsi, 1
    mov rsi, [rsi]
    mov al, 1               ;~
    mov al, 2
    mov [rdi], al
    mov rdi, 1
    call helper
    mov rdi, 2
    mov r8, 1
    jmp .next
.next:
    mov r8, 2
    lea r9, [rax]           ;~
    xor r9d, r9d
    mov r10, 1
    cmp r10, 1
    mov r10, 2
    mov rax, 60
    syscall
helper:
    ret
