global main
extern puts
section .text
main:
    test rdi, rdi
    jz .fail
    push rbp
    call puts
    pop rbp
    ret                     ;~
.fail:
    mov eax, 1
    ret
other:
    call puts
    ret
