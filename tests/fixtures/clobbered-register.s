global _start
extern puts
section .text
_start:
    mov rcx, 3
.loop:
    mov rax, 1
    mov rdi, 1
    mov rsi, msg
    mov rdx, 1
    syscall
    dec rcx                 ;~
    jnz .loop

    mov r12, 3
.ok:
    mov rax, 1
    syscall
    dec r12
    jnz .ok

    mov rsi, msg
    call puts
    mov al, [rsi]           ;~
    mov rdi, rax
    call print
    mov rbx, r11            ;~
    mov rcx, 5
    call quiet
    add rcx, 1
    call print
    mov rcx, 1
    add rcx, 1
    mov rax, 60
    syscall
print:
    mov rax, 1
    syscall
    ret
quiet:
    mov rax, 2
    ret

section .data
msg db "x"
