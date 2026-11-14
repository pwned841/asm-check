section .text
f:
    test rdi, rdi
    jz .done
    inc rax
    jmp .done               ;~
.other:
    inc rbx
.done:
    ret
g:
    jmp f
