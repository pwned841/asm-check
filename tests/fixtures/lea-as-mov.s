section .text
f:
    lea rax, [rbx]          ;~
    lea rax, [rbx + 8]
    lea rax, [rbx*2]
    lea eax, [rbx]
    lea rax, [rel msg]
    ret

section .data
msg db 0
