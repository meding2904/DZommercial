FROM golang:1.27-alpine AS build

WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o /out/azahar-api ./cmd/api

FROM alpine:3.22

RUN apk add --no-cache ca-certificates \
    && addgroup -S azahar \
    && adduser -S -G azahar azahar

COPY --from=build /out/azahar-api /usr/local/bin/azahar-api

USER azahar
EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/azahar-api"]
