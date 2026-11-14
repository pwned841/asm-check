section .text
f:
    add rax, 1              ;~
    add rax, 2
    sub rbx, 4              ;~
    inc rbx
    add rcx, 5              ;~
    sub rcx, 5
    mov rdx, 10             ;~
    add rdx, 5
    inc rsi                 ;~
    dec rsi
    add rdi, 1
    add rdi, 2
    jc .carry
    add r8, 1
    add r9, 2
    ret
.carry:
    ret
