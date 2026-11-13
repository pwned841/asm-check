section .text
f:
    mov rax, [eax]              ;~
    mov [ebx + ecx], al         ;~
    lea eax, [ebx + ecx]
    mov rax, [rax]
    ret
