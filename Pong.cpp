#include <iostream>
#include <cmath>
#include <chrono>
#include <thread>

#ifdef _WIN32
#include <conio.h>
#endif

// 패들 이동 방향 열거형
enum class PaddleDir { NONE, UP, DOWN };

// 순수 C++ 핑퐁 게임 엔진 클래스
class PongGame {
public:
    static const int WIDTH = 32;
    static const int HEIGHT = 16;
    static const int PADDLE_HEIGHT = 4; // 패들 길이 (4픽셀)
    static const int WIN_SCORE = 5;     // 승리 목표 점수

private:
    // 실수형 공 좌표 및 속도 (세밀한 반사각 계산용)
    float ballX, ballY;
    float ballVX, ballVY;

    // 양쪽 패들의 상단 Y 좌표
    int paddle1Y; // 왼쪽 패들 (1P: 조이스틱)
    int paddle2Y; // 오른쪽 패들 (2P: 스마트폰 웹 또는 AI)

    int score1, score2;
    bool twoPlayerMode;
    bool gameOver;
    bool frameBuffer[HEIGHT][WIDTH]; // 32x16 화면 버퍼

public:
    PongGame(bool isTwoPlayer = false) : twoPlayerMode(isTwoPlayer) {
        resetGame();
    }

    // 게임 전체 초기화
    void resetGame() {
        score1 = 0;
        score2 = 0;
        gameOver = false;
        paddle1Y = HEIGHT / 2 - PADDLE_HEIGHT / 2;
        paddle2Y = HEIGHT / 2 - PADDLE_HEIGHT / 2;
        resetBall(1); // 1P 방향으로 서브 시작
        updateFrameBuffer();
    }

    // 1인용 / 2인용 모드 전환
    void setTwoPlayerMode(bool enable) {
        twoPlayerMode = enable;
    }

    // 득점 후 공 위치 및 속도 초기화 (serveDir: -1이면 왼쪽, 1이면 오른쪽으로 서브)
    void resetBall(int serveDir) {
        ballX = WIDTH / 2.0f;
        ballY = HEIGHT / 2.0f;
        ballVX = 0.8f * serveDir; // 수평 기본 속도
        ballVY = 0.2f;            // 초기 약간의 대각선 궤적
    }

    // 1P(왼쪽) 패들 조작
    void movePaddle1(PaddleDir dir) {
        if (dir == PaddleDir::UP && paddle1Y > 0) paddle1Y--;
        else if (dir == PaddleDir::DOWN && paddle1Y + PADDLE_HEIGHT < HEIGHT) paddle1Y++;
    }

    // 2P(오른쪽) 패들 조작 (2인용 모드일 때만 작동)
    void movePaddle2(PaddleDir dir) {
        if (!twoPlayerMode) return;
        if (dir == PaddleDir::UP && paddle2Y > 0) paddle2Y--;
        else if (dir == PaddleDir::DOWN && paddle2Y + PADDLE_HEIGHT < HEIGHT) paddle2Y++;
    }

    // 1프레임 물리 연산 및 충돌 판정
    void update() {
        if (gameOver) return;

        // 1인용 모드일 경우 오른쪽 패들(2P) AI 자동 제어
        if (!twoPlayerMode) {
            float paddleCenter = paddle2Y + PADDLE_HEIGHT / 2.0f;
            if (ballVX > 0) { // 공이 오른쪽으로 올 때만 반응
                if (ballY < paddleCenter - 0.5f) {
                    if (paddle2Y > 0) paddle2Y--;
                } else if (ballY > paddleCenter + 0.5f) {
                    if (paddle2Y + PADDLE_HEIGHT < HEIGHT) paddle2Y++;
                }
            }
        }

        // 공 이동
        ballX += ballVX;
        ballY += ballVY;

        // 1. 상하 벽면 충돌 및 반사
        if (ballY <= 0.0f) {
            ballY = 0.0f;
            ballVY = -ballVY;
        } else if (ballY >= HEIGHT - 1.0f) {
            ballY = HEIGHT - 1.0f;
            ballVY = -ballVY;
        }

        int bX = static_cast<int>(std::round(ballX));
        int bY = static_cast<int>(std::round(ballY));

        // 2. 왼쪽 패들(1P, X=1) 충돌 및 반사각 계산
        if (ballVX < 0 && bX <= 1 && bX >= 0) {
            if (bY >= paddle1Y && bY < paddle1Y + PADDLE_HEIGHT) {
                ballX = 1.0f;
                ballVX = -ballVX; // 수평 방향
