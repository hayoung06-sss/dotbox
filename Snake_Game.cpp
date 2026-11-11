#include <iostream>
#include <vector>
#include <deque>
#include <random>
#include <chrono>
#include <thread>

#ifdef _WIN32
#include <conio.h>
#endif

// 방향 열거형
enum class Direction { UP, DOWN, LEFT, RIGHT };

// 2차원 좌표 구조체
struct Point {
    int x, y;
    bool operator==(const Point& other) const {
        return x == other.x && y == other.y;
    }
};

// 순수 C++ 뱀 게임 엔진 클래스
class SnakeGame {
public:
    static const int WIDTH = 32;
    static const int HEIGHT = 16;

private:
    std::deque<Point> snake;
    Point food;
    Direction dir;
    Direction nextDir;
    bool gameOver;
    int score;
    bool frameBuffer[HEIGHT][WIDTH]; // 32x16 화면 버퍼
    std::mt19937 rng;

public:
    SnakeGame() : rng(std::random_device{}()) {
        reset();
    }

    // 게임 초기화
    void reset() {
        snake.clear();
        snake.push_back({10, 8});
        snake.push_back({9, 8});
        snake.push_back({8, 8});
        dir = Direction::RIGHT;
        nextDir = Direction::RIGHT;
        gameOver = false;
        score = 0;
        spawnFood();
        updateFrameBuffer();
    }

    // 방향 입력 처리 (정반대 방향 전환 방지)
    void setDirection(Direction d) {
        if (dir == Direction::UP && d == Direction::DOWN) return;
        if (dir == Direction::DOWN && d == Direction::UP) return;
        if (dir == Direction::LEFT && d == Direction::RIGHT) return;
        if (dir == Direction::RIGHT && d == Direction::LEFT) return;
        nextDir = d;
    }

    // 먹이 무작위 생성
    void spawnFood() {
        std::uniform_int_distribution<int> distX(0, WIDTH - 1);
        std::uniform_int_distribution<int> distY(0, HEIGHT - 1);
        while (true) {
            Point p = {distX(rng), distY(rng)};
            bool onSnake = false;
            for (const auto& body : snake) {
                if (body == p) {
                    onSnake = true;
                    break;
                }
            }
            if (!onSnake) {
                food = p;
                break;
            }
        }
    }

    // 1프레임 게임 로직 진행
    void update() {
        if (gameOver) return;

        dir = nextDir;
        Point head = snake.front();

        switch (dir) {
            case Direction::UP:    head.y--; break;
            case Direction::DOWN:  head.y++; break;
            case Direction::LEFT:  head.x--; break;
            case Direction::RIGHT: head.x++; break;
        }

        // 1. 벽 충돌 판정
        if (head.x < 0 || head.x >= WIDTH || head.y < 0 || head.y >= HEIGHT) {
            gameOver = true;
            return;
        }

        // 2. 자기 몸통 충돌 판정
        for (const auto& body : snake) {
            if (head == body) {
                gameOver = true;
                return;
            }
        }

        // 머리 추가
        snake.push_front(head);

        // 3. 먹이 섭취 판정
        if (head == food) {
            score += 10;
            spawnFood();
        } else {
            snake.pop_back(); // 먹이를 안 먹었으면 꼬리 이동
        }

        updateFrameBuffer();
    }

    // 32x16 프레임버퍼 갱신
    void updateFrameBuffer() {
        for (int y = 0; y < HEIGHT; ++y) {
            for (int x = 0; x < WIDTH; ++x) {
                frameBuffer[y][x] = false;
            }
        }
        frameBuffer[food.y][food.x] = true;
        for (const auto& body : snake) {
            frameBuffer[body.y][body.x] = true;
        }
    }

    // 외부 렌더러용 상태 조회 함수
    bool getPixel(int x, int y) const { return frameBuffer[y][x]; }
    bool isGameOver() const { return gameOver; }
    int getScore() const { return score; }
    Point getFood() const { return food; }
    Point getHead() const { return snake.front(); }
};

// 콘솔 화면 출력 함수 (PC 테스트용)
void renderToConsole(const SnakeGame& game) {
    std::cout << "\033[H"; // 커서를 좌측 상단으로 이동 (깜빡임 방지)
    std::cout << "=== DotBox 32x16 Snake Engine (Score: " << game.getScore() << ") ===\n";
    
    for (int x = 0; x < SnakeGame::WIDTH + 2; ++x) std::cout << "# ";
    std::cout << "\n";

    for (int y = 0; y < SnakeGame::HEIGHT; ++y) {
        std::cout << "# ";
        for (int x = 0; x < SnakeGame::WIDTH; ++x) {
            if (Point{x, y} == game.getHead()) std::cout << "@ ";
            else if (Point{x, y} == game.getFood()) std::cout << "* ";
            else if (game.getPixel(x, y)) std::cout << "O ";
            else std::cout << ". ";
        }
        std::cout << "#\n";
    }

    for (int x = 0; x < SnakeGame::WIDTH + 2; ++x) std::cout << "# ";
    std::cout << "\n[조작] W/A/S/D: 이동, Q: 종료\n";
}

int main() {
    SnakeGame game;
    std::cout << "\033[2J"; // 콘솔 화면 초기화

    while (!game.isGameOver()) {
        // Windows 환경 실시간 키보드 입력 처리
#ifdef _WIN32
        if (_kbhit()) {
            char ch = _getch();
            if (ch == 'w' || ch == 'W') game.setDirection(Direction::UP);
            else if (ch == 's' || ch == 'S') game.setDirection(Direction::DOWN);
            else if (ch == 'a' || ch == 'A') game.setDirection(Direction::LEFT);
            else if (ch == 'd' || ch == 'D') game.setDirection(Direction::RIGHT);
            else if (ch == 'q' || ch == 'Q') break;
        }
#endif
        game.update();
        renderToConsole(game);
        std::this_thread::sleep_for(std::chrono::milliseconds(150));
    }

    std::cout << "\nGame Over! 최종 점수: " << game.getScore() << "\n";
    return 0;
}
