global main, aligned, _start
extern printf, puts, exit
section .text
main:
    mov rdi, fmt
    call printf             ;~
    push rbp
    call printf
    pop rbp
    ret
aligned:
    sub rsp, 8
    call puts
    add rsp, 8
    call local
    ret
local:
    ret
_start:
    call puts
    push rax
    call puts               ;~
    and rsp, -16
    call puts
    call exit

section .data
fmt db "x", 0
