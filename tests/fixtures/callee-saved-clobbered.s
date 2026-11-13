global good, bad, viahelper, main, _start
section .text
good:
    push rbx
    mov rbx, rdi
    pop rbx
    ret
bad:
    mov rbx, rdi            ;~
    mov r12, rsi            ;~
    mov rax, rbx
    ret
viahelper:
    call helper             ;~
    ret
helper:
    mov r15, 1
    ret
main:
    push rbp
    mov rbp, rsp
    mov r13, 1              ;~
    leave
    ret
_start:
    mov rbx, 1
    mov rax, 60
    syscall
internal:
    mov r14, 2
    ret
