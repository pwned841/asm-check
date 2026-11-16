section .text
f:
    mov rax, [bx]               ;~
    mov rax, [rax + cl]         ;~
    mov rax, [rax + ebx]        ;~
    mov rax, [rax + rbx + rcx]  ;~
    mov rax, [rax + rbx*3]      ;~
    mov rax, [rax + rsp*2]      ;~
    mov rax, [rax*2 + rbx*4]    ;~
    mov rax, [rbx + rcx*8 + 16]
    mov rax, [rax*3]
    mov rax, [rsp + 8]
    lea rax, [rax + rax*4]
    mov rax, [rsp + rsp]        ;~
    mov rax, [rax + rsp]
    mov eax, [ebx + ecx*4]
    ret
