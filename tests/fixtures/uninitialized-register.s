global _start
section .text
_start:
    add rbx, 1              ;~
    mov rax, rcx            ;~
    push rbp
    xor edx, edx
    mov rdi, [rsp + 8]
    mov eax, 1
    syscall                 ;~
    mov r8, 5
    call helper
    call needs_r9           ;~
    mov eax, 60
    syscall
helper:
    mov rax, r8
    ret
needs_r9:
    mov rax, r9
    ret
main:
    mov rax, rdi
    add rax, rcx
    ret
