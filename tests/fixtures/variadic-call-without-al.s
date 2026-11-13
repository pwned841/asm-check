global main
extern printf, puts, scanf
section .text
main:
    push rbp
    lea rdi, [rel fmt]
    call printf             ;~
    lea rdi, [rel fmt]
    call printf             ;~
    xor eax, eax
    lea rdi, [rel fmt]
    call printf
    call puts
    mov al, 0
    call scanf
    pop rbp
    ret

section .data
fmt db "x", 0
