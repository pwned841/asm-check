global balanced, leaky, overpop, framed, branchy, cleaned
section .text
balanced:
    push rbx
    push r12
    pop r12
    pop rbx
    ret
leaky:
    push rbx
    mov rbx, rdi
    ret                     ;~
overpop:
    pop rax
    ret                     ;~
framed:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    push rax
    leave
    ret
branchy:
    push rbx
    test rdi, rdi
    jz .out
    pop rbx
    ret
.out:
    ret                     ;~
cleaned:
    sub rsp, 24
    add rsp, 24
    ret
dynamic:
    sub rsp, rax
    ret
