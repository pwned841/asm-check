section .text
f:
    mov rbx, rax            ;~
    add rbx, rcx
    mov rdx, rsi            ;~
    add rdx, 16
    mov r8, rdi             ;~
    sub r8, 8
    mov r9d, eax            ;~
    add r9d, ecx
    mov rax, rbx            ;~
    add rax, rax
    mov r10, r11
    add r10, 1
    jc .x
    mov rcx, rdx
    sub rcx, rax
    ret
.x:
    ret
