FROM alpine:3.20
RUN apk add --no-cache curl
ENV PORT=8080
CMD ["sh", "-c", "echo hi"]
